mod cli;
mod event_processor;
mod event_processor_with_human_output;
mod event_processor_with_json_output;

use std::io::IsTerminal;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

pub use cli::Cli;
use codex_core::BUILT_IN_OSS_MODEL_PROVIDER_ID;
use codex_core::codex_wrapper::CodexConversation;
use codex_core::codex_wrapper::{self};
use codex_core::config::Config;
use codex_core::config::ConfigOverrides;
use codex_core::config_types::SandboxMode;
use codex_core::protocol::AskForApproval;
use codex_core::protocol::Event;
use codex_core::protocol::EventMsg;
use codex_core::protocol::InputItem;
use codex_core::protocol::Op;
use codex_core::protocol::SandboxPolicy;
use codex_core::protocol::TaskCompleteEvent;
use codex_core::util::is_inside_git_repo;
use codex_ollama::DEFAULT_OSS_MODEL;
use event_processor_with_human_output::EventProcessorWithHumanOutput;
use event_processor_with_json_output::EventProcessorWithJsonOutput;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::event_processor::CodexStatus;
use crate::event_processor::EventProcessor;

pub async fn run_main(cli: Cli, codex_linux_sandbox_exe: Option<PathBuf>) -> anyhow::Result<()> {
    let Cli {
        images,
        model: model_cli_arg,
        oss,
        config_profile,
        full_auto,
        dangerously_bypass_approvals_and_sandbox,
        cwd,
        skip_git_repo_check,
        color,
        last_message_file,
        json: json_mode,
        sandbox_mode: sandbox_mode_cli_arg,
        approval_policy: approval_policy_cli_arg,
        resume,
        prompt,
        config_overrides,
    } = cli;

    // Determine the prompt based on CLI arg and/or stdin.
    let prompt = match prompt {
        Some(p) if p != "-" => p,
        // Either `-` was passed or no positional arg.
        maybe_dash => {
            // When no arg (None) **and** stdin is a TTY, bail out early – unless the
            // user explicitly forced reading via `-`.
            let force_stdin = matches!(maybe_dash.as_deref(), Some("-"));

            if std::io::stdin().is_terminal() && !force_stdin {
                eprintln!(
                    "No prompt provided. Either specify one as an argument or pipe the prompt into stdin."
                );
                std::process::exit(1);
            }

            // Ensure the user knows we are waiting on stdin, as they may
            // have gotten into this state by mistake. If so, and they are not
            // writing to stdin, Codex will hang indefinitely, so this should
            // help them debug in that case.
            if !force_stdin {
                eprintln!("Reading prompt from stdin...");
            }
            let mut buffer = String::new();
            if let Err(e) = std::io::stdin().read_to_string(&mut buffer) {
                eprintln!("Failed to read prompt from stdin: {e}");
                std::process::exit(1);
            } else if buffer.trim().is_empty() {
                eprintln!("No prompt provided via stdin.");
                std::process::exit(1);
            }
            buffer
        }
    };

    let (stdout_with_ansi, stderr_with_ansi) = match color {
        cli::Color::Always => (true, true),
        cli::Color::Never => (false, false),
        cli::Color::Auto => (
            std::io::stdout().is_terminal(),
            std::io::stderr().is_terminal(),
        ),
    };

    // TODO(mbolin): Take a more thoughtful approach to logging.
    let default_level = "error";
    let _ = tracing_subscriber::fmt()
        // Fallback to the `default_level` log filter if the environment
        // variable is not set _or_ contains an invalid value
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .or_else(|_| EnvFilter::try_new(default_level))
                .unwrap_or_else(|_| EnvFilter::new(default_level)),
        )
        .with_ansi(stderr_with_ansi)
        .with_writer(std::io::stderr)
        .try_init();

    let sandbox_mode = if full_auto {
        Some(SandboxMode::WorkspaceWrite)
    } else if dangerously_bypass_approvals_and_sandbox {
        Some(SandboxMode::DangerFullAccess)
    } else {
        sandbox_mode_cli_arg.map(Into::<SandboxMode>::into)
    };

    // When using `--oss`, let the bootstrapper pick the model (defaulting to
    // gpt-oss:20b) and ensure it is present locally. Also, force the built‑in
    // `oss` model provider.
    let model = if let Some(model) = model_cli_arg {
        Some(model)
    } else if oss {
        Some(DEFAULT_OSS_MODEL.to_owned())
    } else {
        None // No model specified, will use the default.
    };

    let model_provider = if oss {
        Some(BUILT_IN_OSS_MODEL_PROVIDER_ID.to_string())
    } else {
        None // No specific model provider override.
    };

    // Load configuration and determine approval policy
    let overrides = ConfigOverrides {
        model,
        config_profile,
        // Use CLI argument if provided, otherwise default to Never for headless operation
        approval_policy: approval_policy_cli_arg
            .map(|a| a.into())
            .or_else(|| Some(AskForApproval::Never)),
        sandbox_mode,
        cwd: cwd.map(|p| p.canonicalize().unwrap_or(p)),
        model_provider,
        codex_linux_sandbox_exe,
        base_instructions: None,
        include_plan_tool: None,
        disable_response_storage: oss.then_some(true),
        show_raw_agent_reasoning: oss.then_some(true),
    };
    // Parse `-c` overrides.
    let cli_kv_overrides = match config_overrides.parse_overrides() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error parsing -c overrides: {e}");
            std::process::exit(1);
        }
    };

    let config = Config::load_with_cli_overrides(cli_kv_overrides, overrides)?;
    let mut event_processor: Box<dyn EventProcessor> = if json_mode {
        Box::new(EventProcessorWithJsonOutput::new(last_message_file.clone()))
    } else {
        Box::new(EventProcessorWithHumanOutput::create_with_ansi(
            stdout_with_ansi,
            &config,
            last_message_file.clone(),
        ))
    };

    if oss {
        codex_ollama::ensure_oss_ready(&config)
            .await
            .map_err(|e| anyhow::anyhow!("OSS setup failed: {e}"))?;
    }

    // Print the effective configuration and prompt so users can see what Codex
    // is using.
    event_processor.print_config_summary(&config, &prompt);

    if !skip_git_repo_check && !is_inside_git_repo(&config.cwd.to_path_buf()) {
        eprintln!("Not inside a trusted directory and --skip-git-repo-check was not specified.");
        std::process::exit(1);
    }

    // If --resume is specified, load conversation from rollout
    let resume_history = if resume {
        match codex_core::rollout::find_latest_rollout(&config).await? {
            Some(rollout_path) => {
                eprintln!("Loading conversation from: {}", rollout_path.display());
                match codex_core::rollout::load_rollout_conversation(&rollout_path).await {
                    Ok(conversation) => {
                        if !conversation.is_empty() {
                            eprintln!("Loaded {} messages from previous session", conversation.len());
                            conversation
                        } else {
                            eprintln!("Warning: No conversation found in rollout file");
                            Vec::new()
                        }
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to load rollout: {}", e);
                        Vec::new()
                    }
                }
            }
            None => {
                eprintln!("Warning: No previous session found to resume");
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    // Store sandbox_policy before moving config
    let sandbox_policy = config.sandbox_policy.clone();
    
    let CodexConversation {
        codex: codex_wrapper,
        session_configured,
        ctrl_c,
        ..
    } = codex_wrapper::init_codex(config).await?;
    let codex = Arc::new(codex_wrapper);
    info!("Codex initialized with event: {session_configured:?}");

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Event>();
    {
        let codex = codex.clone();
        tokio::spawn(async move {
            loop {
                let interrupted = ctrl_c.notified();
                tokio::select! {
                    _ = interrupted => {
                        // Forward an interrupt to the codex so it can abort any in‑flight task.
                        let _ = codex
                            .submit(
                                Op::Interrupt,
                            )
                            .await;

                        // Exit the inner loop and return to the main input prompt.  The codex
                        // will emit a `TurnInterrupted` (Error) event which is drained later.
                        break;
                    }
                    res = codex.next_event() => match res {
                        Ok(event) => {
                            debug!("Received event: {event:?}");

                            let is_shutdown_complete = matches!(event.msg, EventMsg::ShutdownComplete);
                            if let Err(e) = tx.send(event) {
                                error!("Error sending event: {e:?}");
                                break;
                            }
                            if is_shutdown_complete {
                                info!("Received shutdown event, exiting event loop.");
                                break;
                            }
                        },
                        Err(e) => {
                            error!("Error receiving event: {e:?}");
                            break;
                        }
                    }
                }
            }
        });
    }

    // Track if we need to send a new prompt after history replay
    let mut should_send_new_prompt = true;
    let mut task_already_complete = false;
    
    // If resuming, send the history first
    if !resume_history.is_empty() {
        eprintln!("Replaying conversation history...");
        
        // Combine history with the new prompt as a single submission
        // This prevents duplicate processing
        let mut combined_items = Vec::new();
        
        // Add system message to explicitly inform about available tools
        // This ensures the model knows it can use shell commands even during resume
        match &sandbox_policy {
            SandboxPolicy::WorkspaceWrite { network_access, .. } => {
                if *network_access {
                    combined_items.push(InputItem::Text {
                        text: "System: You have access to the shell tool with network access enabled. You can use curl, wget and other network commands.".to_string()
                    });
                } else {
                    combined_items.push(InputItem::Text {
                        text: "System: You have access to the shell tool. Network commands like curl require escalated permissions.".to_string()
                    });
                }
            }
            SandboxPolicy::DangerFullAccess => {
                combined_items.push(InputItem::Text {
                    text: "System: You have full shell access with no restrictions.".to_string()
                });
            }
            SandboxPolicy::ReadOnly => {
                combined_items.push(InputItem::Text {
                    text: "System: You have access to the shell tool in read-only mode. Write operations require escalated permissions.".to_string()
                });
            }
        }
        
        // Add the history
        combined_items.extend(resume_history);
        
        // Add the new prompt
        combined_items.push(InputItem::Text { text: prompt.clone() });
        
        let combined_task_id = codex.submit(Op::UserInput { items: combined_items }).await?;
        info!("Sent combined history and new prompt with event ID: {combined_task_id}");
        
        // Mark that we've already sent the prompt
        should_send_new_prompt = false;
        
        // Process events and display them
        while let Some(event) = rx.recv().await {
            let is_task_complete = event.id == combined_task_id
                && matches!(
                    event.msg,
                    EventMsg::TaskComplete(TaskCompleteEvent {
                        last_agent_message: _,
                    })
                );
            
            // Process the event through the event processor to display it
            let shutdown = event_processor.process_event(event);
            
            if is_task_complete {
                // Task is complete, mark it and exit
                task_already_complete = true;
                break;
            }
            
            match shutdown {
                CodexStatus::Running => continue,
                CodexStatus::InitiateShutdown => {
                    codex.submit(Op::Shutdown).await?;
                    task_already_complete = true;
                }
                CodexStatus::Shutdown => {
                    task_already_complete = true;
                    break;
                }
            }
        }
    }

    // Send images first, if any.
    if !images.is_empty() {
        let items: Vec<InputItem> = images
            .into_iter()
            .map(|path| InputItem::LocalImage { path })
            .collect();
        let initial_images_event_id = codex.submit(Op::UserInput { items }).await?;
        info!("Sent images with event ID: {initial_images_event_id}");
        while let Some(event) = rx.recv().await {
            if event.id == initial_images_event_id
                && matches!(
                    event.msg,
                    EventMsg::TaskComplete(TaskCompleteEvent {
                        last_agent_message: _,
                    })
                )
            {
                break;
            }
        }
    }

    // Send the prompt only if we haven't already sent it with history
    if should_send_new_prompt && !task_already_complete {
        let items: Vec<InputItem> = vec![InputItem::Text { text: prompt }];
        let initial_prompt_task_id = codex.submit(Op::UserInput { items }).await?;
        info!("Sent prompt with event ID: {initial_prompt_task_id}");
    }

    // Run the loop until the task is complete (only if not already complete)
    if !task_already_complete {
        while let Some(event) = rx.recv().await {
            let shutdown: CodexStatus = event_processor.process_event(event);
            match shutdown {
                CodexStatus::Running => continue,
                CodexStatus::InitiateShutdown => {
                    codex.submit(Op::Shutdown).await?;
                }
                CodexStatus::Shutdown => {
                    break;
                }
            }
        }
    }

    Ok(())
}
