use crate::bash::try_parse_bash;
use crate::bash::try_parse_word_only_commands_sequence;
use serde::Deserialize;
use serde::Serialize;
use shlex::split as shlex_split;
use shlex::try_join as shlex_try_join;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum ParsedCommand {
    Read {
        cmd: String,
        name: String,
    },
    ListFiles {
        cmd: String,
        path: Option<String>,
    },
    Search {
        cmd: String,
        query: Option<String>,
        path: Option<String>,
    },
    Format {
        cmd: String,
        tool: Option<String>,
        targets: Option<Vec<String>>,
    },
    Test {
        cmd: String,
    },
    Lint {
        cmd: String,
        tool: Option<String>,
        targets: Option<Vec<String>>,
    },
    Noop {
        cmd: String,
    },
    Unknown {
        cmd: String,
    },
}

fn shlex_join(tokens: &[String]) -> String {
    shlex_try_join(tokens.iter().map(|s| s.as_str()))
        .unwrap_or_else(|_| "<command included NUL byte>".to_string())
}

/// DO NOT REVIEW THIS CODE BY HAND
/// This parsing code is quite complex and not easy to hand-modify.
/// The easiest way to iterate is to add unit tests and have Codex fix the implementation.
/// To encourage this, the tests have been put directly below this function rather than at the bottom of the
///
/// Parses metadata out of an arbitrary command.
/// These commands are model driven and could include just about anything.
/// The parsing is slightly lossy due to the ~infinite expressiveness of an arbitrary command.
/// The goal of the parsed metadata is to be able to provide the user with a human readable gis
/// of what it is doing.
pub fn parse_command(command: &[String]) -> Vec<ParsedCommand> {
    // Parse and then collapse consecutive duplicate commands to avoid redundant summaries.
    let parsed = parse_command_impl(command);
    let mut deduped: Vec<ParsedCommand> = Vec::with_capacity(parsed.len());
    for cmd in parsed.into_iter() {
        if deduped.last().is_some_and(|prev| prev == &cmd) {
            continue;
        }
        deduped.push(cmd);
    }
    deduped
}

pub fn parse_command_impl(command: &[String]) -> Vec<ParsedCommand> {
    if let Some(commands) = parse_bash_lc_commands(command) {
        return commands;
    }

    let normalized = normalize_tokens(command);

    let parts = if contains_connectors(&normalized) {
        split_on_connectors(&normalized)
    } else {
        vec![normalized.clone()]
    };

    // Preserve left-to-right execution order for all commands, including bash -c/-lc
    // so summaries reflect the order they will run.

    // Map each pipeline segment to its parsed summary.
    let mut commands: Vec<ParsedCommand> = parts
        .iter()
        .map(|tokens| summarize_main_tokens(tokens))
        .collect();

    while let Some(next) = simplify_once(&commands) {
        commands = next;
    }

    commands
}

fn simplify_once(commands: &[ParsedCommand]) -> Option<Vec<ParsedCommand>> {
    if commands.len() <= 1 {
        return None;
    }

    // echo ... && ...rest => ...rest
    if let ParsedCommand::Unknown { cmd } = &commands[0] {
        if shlex_split(cmd).is_some_and(|t| t.first().map(|s| s.as_str()) == Some("echo")) {
            return Some(commands[1..].to_vec());
        }
    }

    // cd foo && [any Test command] => [any Test command]
    if let Some(idx) = commands.iter().position(|pc| match pc {
        ParsedCommand::Unknown { cmd } => {
            shlex_split(cmd).is_some_and(|t| t.first().map(|s| s.as_str()) == Some("cd"))
        }
        _ => false,
    }) {
        if commands
            .iter()
            .skip(idx + 1)
            .any(|pc| matches!(pc, ParsedCommand::Test { .. }))
        {
            let mut out = Vec::with_capacity(commands.len() - 1);
            out.extend_from_slice(&commands[..idx]);
            out.extend_from_slice(&commands[idx + 1..]);
            return Some(out);
        }
    }

    // cmd || true => cmd
    if let Some(idx) = commands.iter().position(|pc| match pc {
        ParsedCommand::Noop { cmd } => cmd == "true",
        _ => false,
    }) {
        let mut out = Vec::with_capacity(commands.len() - 1);
        out.extend_from_slice(&commands[..idx]);
        out.extend_from_slice(&commands[idx + 1..]);
        return Some(out);
    }

    // nl -[any_flags] && ...rest => ...rest
    if let Some(idx) = commands.iter().position(|pc| match pc {
        ParsedCommand::Unknown { cmd } => {
            if let Some(tokens) = shlex_split(cmd) {
                tokens.first().is_some_and(|s| s.as_str() == "nl")
                    && tokens.iter().skip(1).all(|t| t.starts_with('-'))
            } else {
                false
            }
        }
        _ => false,
    }) {
        let mut out = Vec::with_capacity(commands.len() - 1);
        out.extend_from_slice(&commands[..idx]);
        out.extend_from_slice(&commands[idx + 1..]);
        return Some(out);
    }

    None
}

/// Validates that this is a `sed -n 123,123p` command.
fn is_valid_sed_n_arg(arg: Option<&str>) -> bool {
    let s = match arg {
        Some(s) => s,
        None => return false,
    };
    let core = match s.strip_suffix('p') {
        Some(rest) => rest,
        None => return false,
    };
    let parts: Vec<&str> = core.split(',').collect();
    match parts.as_slice() {
        [num] => !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()),
        [a, b] => {
            !a.is_empty()
                && !b.is_empty()
                && a.chars().all(|c| c.is_ascii_digit())
                && b.chars().all(|c| c.is_ascii_digit())
        }
        _ => false,
    }
}

/// Normalize a command by:
/// - Removing `yes`/`no`/`bash -c`/`bash -lc` prefixes.
/// - Splitting on `|` and `&&`/`||`/`;
fn normalize_tokens(cmd: &[String]) -> Vec<String> {
    match cmd {
        [first, pipe, rest @ ..] if (first == "yes" || first == "y") && pipe == "|" => {
            // Do not re-shlex already-tokenized input; just drop the prefix.
            rest.to_vec()
        }
        [first, pipe, rest @ ..] if (first == "no" || first == "n") && pipe == "|" => {
            // Do not re-shlex already-tokenized input; just drop the prefix.
            rest.to_vec()
        }
        [bash, flag, script] if bash == "bash" && (flag == "-c" || flag == "-lc") => {
            shlex_split(script)
                .unwrap_or_else(|| vec!["bash".to_string(), flag.clone(), script.clone()])
        }
        _ => cmd.to_vec(),
    }
}

fn contains_connectors(tokens: &[String]) -> bool {
    tokens
        .iter()
        .any(|t| t == "&&" || t == "||" || t == "|" || t == ";")
}

fn split_on_connectors(tokens: &[String]) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    for t in tokens {
        if t == "&&" || t == "||" || t == "|" || t == ";" {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(t.clone());
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn trim_at_connector(tokens: &[String]) -> Vec<String> {
    let idx = tokens
        .iter()
        .position(|t| t == "|" || t == "&&" || t == "||" || t == ";")
        .unwrap_or(tokens.len());
    tokens[..idx].to_vec()
}

/// Shorten a path to the last component, excluding `build`/`dist`/`node_modules`/`src`.
/// It also pulls out a useful path from a directory such as:
/// - webview/src -> webview
/// - foo/src/ -> foo
/// - packages/app/node_modules/ -> app
fn short_display_path(path: &str) -> String {
    // Normalize separators and drop any trailing slash for display.
    let normalized = path.replace('\\', "/");
    let trimmed = normalized.trim_end_matches('/');
    let mut parts = trimmed.split('/').rev().filter(|p| {
        !p.is_empty() && *p != "build" && *p != "dist" && *p != "node_modules" && *p != "src"
    });
    parts
        .next()
        .map(|s| s.to_string())
        .unwrap_or_else(|| trimmed.to_string())
}

// Skip values consumed by specific flags and ignore --flag=value style arguments.
fn skip_flag_values<'a>(args: &'a [String], flags_with_vals: &[&str]) -> Vec<&'a String> {
    let mut out: Vec<&'a String> = Vec::new();
    let mut skip_next = false;
    for (i, a) in args.iter().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }
        if a == "--" {
            // From here on, everything is positional operands; push the rest and break.
            for rest in &args[i + 1..] {
                out.push(rest);
            }
            break;
        }
        if a.starts_with("--") && a.contains('=') {
            // --flag=value form: treat as a flag taking a value; skip entirely.
            continue;
        }
        if flags_with_vals.contains(&a.as_str()) {
            // This flag consumes the next argument as its value.
            if i + 1 < args.len() {
                skip_next = true;
            }
            continue;
        }
        out.push(a);
    }
    out
}

/// Common flags for ESLint that take a following value and should not be
/// considered positional targets.
const ESLINT_FLAGS_WITH_VALUES: &[&str] = &[
    "-c",
    "--config",
    "--parser",
    "--parser-options",
    "--rulesdir",
    "--plugin",
    "--max-warnings",
    "--format",
];

fn collect_non_flag_targets(args: &[String]) -> Option<Vec<String>> {
    let mut targets = Vec::new();
    let mut skip_next = false;
    for (i, a) in args.iter().enumerate() {
        if a == "--" {
            break;
        }
        if skip_next {
            skip_next = false;
            continue;
        }
        if a == "-p"
            || a == "--package"
            || a == "--features"
            || a == "-C"
            || a == "--config"
            || a == "--config-path"
            || a == "--out-dir"
            || a == "-o"
            || a == "--run"
            || a == "--max-warnings"
            || a == "--format"
        {
            if i + 1 < args.len() {
                skip_next = true;
            }
            continue;
        }
        if a.starts_with('-') {
            continue;
        }
        targets.push(a.clone());
    }
    if targets.is_empty() {
        None
    } else {
        Some(targets)
    }
}

fn collect_non_flag_targets_with_flags(
    args: &[String],
    flags_with_vals: &[&str],
) -> Option<Vec<String>> {
    let targets: Vec<String> = skip_flag_values(args, flags_with_vals)
        .into_iter()
        .filter(|a| !a.starts_with('-'))
        .cloned()
        .collect();
    if targets.is_empty() {
        None
    } else {
        Some(targets)
    }
}

fn is_pathish(s: &str) -> bool {
    s == "."
        || s == ".."
        || s.starts_with("./")
        || s.starts_with("../")
        || s.contains('/')
        || s.contains('\\')
}

fn parse_fd_query_and_path(tail: &[String]) -> (Option<String>, Option<String>) {
    let args_no_connector = trim_at_connector(tail);
    // fd has several flags that take values (e.g., -t/--type, -e/--extension).
    // Skip those values when extracting positional operands.
    let candidates = skip_flag_values(
        &args_no_connector,
        &[
            "-t",
            "--type",
            "-e",
            "--extension",
            "-E",
            "--exclude",
            "--search-path",
        ],
    );
    let non_flags: Vec<&String> = candidates
        .into_iter()
        .filter(|p| !p.starts_with('-'))
        .collect();
    match non_flags.as_slice() {
        [one] => {
            if is_pathish(one) {
                (None, Some(short_display_path(one)))
            } else {
                (Some((*one).clone()), None)
            }
        }
        [q, p, ..] => (Some((*q).clone()), Some(short_display_path(p))),
        _ => (None, None),
    }
}

fn parse_find_query_and_path(tail: &[String]) -> (Option<String>, Option<String>) {
    let args_no_connector = trim_at_connector(tail);
    // First positional argument (excluding common unary operators) is the root path
    let mut path: Option<String> = None;
    for a in &args_no_connector {
        if !a.starts_with('-') && *a != "!" && *a != "(" && *a != ")" {
            path = Some(short_display_path(a));
            break;
        }
    }
    // Extract a common name/path/regex pattern if present
    let mut query: Option<String> = None;
    let mut i = 0;
    while i < args_no_connector.len() {
        let a = &args_no_connector[i];
        if a == "-name" || a == "-iname" || a == "-path" || a == "-regex" {
            if i + 1 < args_no_connector.len() {
                query = Some(args_no_connector[i + 1].clone());
            }
            break;
        }
        i += 1;
    }
    (query, path)
}

fn classify_npm_like(tool: &str, tail: &[String], full_cmd: &[String]) -> Option<ParsedCommand> {
    let mut r = tail;
    if tool == "pnpm" && r.first().map(|s| s.as_str()) == Some("-r") {
        r = &r[1..];
    }
    let mut script_name: Option<String> = None;
    if r.first().map(|s| s.as_str()) == Some("run") {
        script_name = r.get(1).cloned();
    } else {
        let is_test_cmd = (tool == "npm" && r.first().map(|s| s.as_str()) == Some("t"))
            || ((tool == "npm" || tool == "pnpm" || tool == "yarn")
                && r.first().map(|s| s.as_str()) == Some("test"));
        if is_test_cmd {
            script_name = Some("test".to_string());
        }
    }
    if let Some(name) = script_name {
        let lname = name.to_lowercase();
        if lname == "test" || lname == "unit" || lname == "jest" || lname == "vitest" {
            return Some(ParsedCommand::Test {
                cmd: shlex_join(full_cmd),
            });
        }
        if lname == "lint" || lname == "eslint" {
            return Some(ParsedCommand::Lint {
                cmd: shlex_join(full_cmd),
                tool: Some(format!("{tool}-script:{name}")),
                targets: None,
            });
        }
        if lname == "format" || lname == "fmt" || lname == "prettier" {
            return Some(ParsedCommand::Format {
                cmd: shlex_join(full_cmd),
                tool: Some(format!("{tool}-script:{name}")),
                targets: None,
            });
        }
    }
    None
}

fn parse_bash_lc_commands(original: &[String]) -> Option<Vec<ParsedCommand>> {
    let [bash, flag, script] = original else {
        return None;
    };
    if bash != "bash" || flag != "-lc" {
        return None;
    }
    if let Some(tree) = try_parse_bash(script) {
        if let Some(all_commands) = try_parse_word_only_commands_sequence(&tree, script) {
            if !all_commands.is_empty() {
                let script_tokens = shlex_split(script)
                    .unwrap_or_else(|| vec!["bash".to_string(), flag.clone(), script.clone()]);
                // Strip small formatting helpers (e.g., head/tail/awk/wc/etc) so we
                // bias toward the primary command when pipelines are present.
                // First, drop obvious small formatting helpers (e.g., wc/awk/etc).
                let had_multiple_commands = all_commands.len() > 1;
                // The bash AST walker yields commands in right-to-left order for
                // connector/pipeline sequences. Reverse to reflect actual execution order.
                let mut filtered_commands = drop_small_formatting_commands(all_commands);
                filtered_commands.reverse();
                if filtered_commands.is_empty() {
                    return Some(vec![ParsedCommand::Unknown {
                        cmd: script.clone(),
                    }]);
                }
                let mut commands: Vec<ParsedCommand> = filtered_commands
                    .into_iter()
                    .map(|tokens| summarize_main_tokens(&tokens))
                    .collect();
                if commands.len() > 1 {
                    commands.retain(|pc| !matches!(pc, ParsedCommand::Noop { .. }));
                }
                if commands.len() == 1 {
                    // If we reduced to a single command, attribute the full original script
                    // for clearer UX in file-reading and listing scenarios, or when there were
                    // no connectors in the original script. For search commands that came from
                    // a pipeline (e.g. `rg --files | sed -n`), keep only the primary command.
                    let had_connectors = had_multiple_commands
                        || script_tokens
                            .iter()
                            .any(|t| t == "|" || t == "&&" || t == "||" || t == ";");
                    commands = commands
                        .into_iter()
                        .map(|pc| match pc {
                            ParsedCommand::Read { name, cmd, .. } => {
                                if had_connectors {
                                    let has_pipe = script_tokens.iter().any(|t| t == "|");
                                    let has_sed_n = script_tokens.windows(2).any(|w| {
                                        w.first().map(|s| s.as_str()) == Some("sed")
                                            && w.get(1).map(|s| s.as_str()) == Some("-n")
                                    });
                                    if has_pipe && has_sed_n {
                                        ParsedCommand::Read {
                                            cmd: script.clone(),
                                            name,
                                        }
                                    } else {
                                        ParsedCommand::Read {
                                            cmd: cmd.clone(),
                                            name,
                                        }
                                    }
                                } else {
                                    ParsedCommand::Read {
                                        cmd: shlex_join(&script_tokens),
                                        name,
                                    }
                                }
                            }
                            ParsedCommand::ListFiles { path, cmd, .. } => {
                                if had_connectors {
                                    ParsedCommand::ListFiles {
                                        cmd: cmd.clone(),
                                        path,
                                    }
                                } else {
                                    ParsedCommand::ListFiles {
                                        cmd: shlex_join(&script_tokens),
                                        path,
                                    }
                                }
                            }
                            ParsedCommand::Search {
                                query, path, cmd, ..
                            } => {
                                if had_connectors {
                                    ParsedCommand::Search {
                                        cmd: cmd.clone(),
                                        query,
                                        path,
                                    }
                                } else {
                                    ParsedCommand::Search {
                                        cmd: shlex_join(&script_tokens),
                                        query,
                                        path,
                                    }
                                }
                            }
                            ParsedCommand::Format {
                                tool, targets, cmd, ..
                            } => ParsedCommand::Format {
                                cmd: cmd.clone(),
                                tool,
                                targets,
                            },
                            ParsedCommand::Test { cmd, .. } => {
                                ParsedCommand::Test { cmd: cmd.clone() }
                            }
                            ParsedCommand::Lint {
                                tool, targets, cmd, ..
                            } => ParsedCommand::Lint {
                                cmd: cmd.clone(),
                                tool,
                                targets,
                            },
                            ParsedCommand::Unknown { .. } => ParsedCommand::Unknown {
                                cmd: script.clone(),
                            },
                            ParsedCommand::Noop { .. } => ParsedCommand::Noop {
                                cmd: script.clone(),
                            },
                        })
                        .collect();
                }
                return Some(commands);
            }
        }
    }
    Some(vec![ParsedCommand::Unknown {
        cmd: script.clone(),
    }])
}

/// Return true if this looks like a small formatting helper in a pipeline.
/// Examples: `head -n 40`, `tail -n +10`, `wc -l`, `awk ...`, `cut ...`, `tr ...`.
/// We try to keep variants that clearly include a file path (e.g. `tail -n 30 file`).
fn is_small_formatting_command(tokens: &[String]) -> bool {
    if tokens.is_empty() {
        return false;
    }
    let cmd = tokens[0].as_str();
    match cmd {
        // Always formatting; typically used in pipes.
        // `nl` is special-cased below to allow `nl <file>` to be treated as a read command.
        "wc" | "tr" | "cut" | "sort" | "uniq" | "xargs" | "tee" | "column" | "awk" | "yes"
        | "printf" => true,
        "head" => {
            // Treat as formatting when no explicit file operand is present.
            // Common forms: `head -n 40`, `head -c 100`.
            // Keep cases like `head -n 40 file`.
            tokens.len() < 3
        }
        "tail" => {
            // Treat as formatting when no explicit file operand is present.
            // Common forms: `tail -n +10`, `tail -n 30`.
            // Keep cases like `tail -n 30 file`.
            tokens.len() < 3
        }
        "sed" => {
            // Keep `sed -n <range> file` (treated as a file read elsewhere);
            // otherwise consider it a formatting helper in a pipeline.
            tokens.len() < 4
                || !(tokens[1] == "-n" && is_valid_sed_n_arg(tokens.get(2).map(|s| s.as_str())))
        }
        _ => false,
    }
}

fn drop_small_formatting_commands(mut commands: Vec<Vec<String>>) -> Vec<Vec<String>> {
    commands.retain(|tokens| !is_small_formatting_command(tokens));
    commands
}

fn summarize_main_tokens(main_cmd: &[String]) -> ParsedCommand {
    match main_cmd.split_first() {
        Some((head, tail)) if head == "true" && tail.is_empty() => ParsedCommand::Noop {
            cmd: shlex_join(main_cmd),
        },
        // (sed-specific logic handled below in dedicated arm returning Read)
        Some((head, tail))
            if head == "cargo" && tail.first().map(|s| s.as_str()) == Some("fmt") =>
        {
            ParsedCommand::Format {
                cmd: shlex_join(main_cmd),
                tool: Some("cargo fmt".to_string()),
                targets: collect_non_flag_targets(&tail[1..]),
            }
        }
        Some((head, tail))
            if head == "cargo" && tail.first().map(|s| s.as_str()) == Some("clippy") =>
        {
            ParsedCommand::Lint {
                cmd: shlex_join(main_cmd),
                tool: Some("cargo clippy".to_string()),
                targets: collect_non_flag_targets(&tail[1..]),
            }
        }
        Some((head, tail))
            if head == "cargo" && tail.first().map(|s| s.as_str()) == Some("test") =>
        {
            ParsedCommand::Test {
                cmd: shlex_join(main_cmd),
            }
        }
        Some((head, tail)) if head == "rustfmt" => ParsedCommand::Format {
            cmd: shlex_join(main_cmd),
            tool: Some("rustfmt".to_string()),
            targets: collect_non_flag_targets(tail),
        },
        Some((head, tail)) if head == "go" && tail.first().map(|s| s.as_str()) == Some("fmt") => {
            ParsedCommand::Format {
                cmd: shlex_join(main_cmd),
                tool: Some("go fmt".to_string()),
                targets: collect_non_flag_targets(&tail[1..]),
            }
        }
        Some((head, tail)) if head == "go" && tail.first().map(|s| s.as_str()) == Some("test") => {
            ParsedCommand::Test {
                cmd: shlex_join(main_cmd),
            }
        }
        Some((head, _)) if head == "pytest" => ParsedCommand::Test {
            cmd: shlex_join(main_cmd),
        },
        Some((head, tail)) if head == "eslint" => {
            // Treat configuration flags with values (e.g. `-c .eslintrc`) as non-targets.
            let targets = collect_non_flag_targets_with_flags(tail, ESLINT_FLAGS_WITH_VALUES);
            ParsedCommand::Lint {
                cmd: shlex_join(main_cmd),
                tool: Some("eslint".to_string()),
                targets,
            }
        }
        Some((head, tail)) if head == "prettier" => ParsedCommand::Format {
            cmd: shlex_join(main_cmd),
            tool: Some("prettier".to_string()),
            targets: collect_non_flag_targets(tail),
        },
        Some((head, tail)) if head == "black" => ParsedCommand::Format {
            cmd: shlex_join(main_cmd),
            tool: Some("black".to_string()),
            targets: collect_non_flag_targets(tail),
        },
        Some((head, tail))
            if head == "ruff" && tail.first().map(|s| s.as_str()) == Some("check") =>
        {
            ParsedCommand::Lint {
                cmd: shlex_join(main_cmd),
                tool: Some("ruff".to_string()),
                targets: collect_non_flag_targets(&tail[1..]),
            }
        }
        Some((head, tail))
            if head == "ruff" && tail.first().map(|s| s.as_str()) == Some("format") =>
        {
            ParsedCommand::Format {
                cmd: shlex_join(main_cmd),
                tool: Some("ruff".to_string()),
                targets: collect_non_flag_targets(&tail[1..]),
            }
        }
        Some((head, _)) if (head == "jest" || head == "vitest") => ParsedCommand::Test {
            cmd: shlex_join(main_cmd),
        },
        Some((head, tail))
            if head == "npx" && tail.first().map(|s| s.as_str()) == Some("eslint") =>
        {
            let targets = collect_non_flag_targets_with_flags(&tail[1..], ESLINT_FLAGS_WITH_VALUES);
            ParsedCommand::Lint {
                cmd: shlex_join(main_cmd),
                tool: Some("eslint".to_string()),
                targets,
            }
        }
        Some((head, tail))
            if head == "npx" && tail.first().map(|s| s.as_str()) == Some("prettier") =>
        {
            ParsedCommand::Format {
                cmd: shlex_join(main_cmd),
                tool: Some("prettier".to_string()),
                targets: collect_non_flag_targets(&tail[1..]),
            }
        }
        // NPM-like scripts including yarn
        Some((tool, tail)) if (tool == "pnpm" || tool == "npm" || tool == "yarn") => {
            if let Some(cmd) = classify_npm_like(tool, tail, main_cmd) {
                cmd
            } else {
                ParsedCommand::Unknown {
                    cmd: shlex_join(main_cmd),
                }
            }
        }
        Some((head, tail)) if head == "ls" => {
            // Avoid treating option values as paths (e.g., ls -I "*.test.js").
            let candidates = skip_flag_values(
                tail,
                &[
                    "-I",
                    "-w",
                    "--block-size",
                    "--format",
                    "--time-style",
                    "--color",
                    "--quoting-style",
                ],
            );
            let path = candidates
                .into_iter()
                .find(|p| !p.starts_with('-'))
                .map(|p| short_display_path(p));
            ParsedCommand::ListFiles {
                cmd: shlex_join(main_cmd),
                path,
            }
        }
        Some((head, tail)) if head == "rg" => {
            let args_no_connector = trim_at_connector(tail);
            let has_files_flag = args_no_connector.iter().any(|a| a == "--files");
            let non_flags: Vec<&String> = args_no_connector
                .iter()
                .filter(|p| !p.starts_with('-'))
                .collect();
            let (query, path) = if has_files_flag {
                (None, non_flags.first().map(|s| short_display_path(s)))
            } else {
                (
                    non_flags.first().cloned().map(|s| s.to_string()),
                    non_flags.get(1).map(|s| short_display_path(s)),
                )
            };
            ParsedCommand::Search {
                cmd: shlex_join(main_cmd),
                query,
                path,
            }
        }
        Some((head, tail)) if head == "fd" => {
            let (query, path) = parse_fd_query_and_path(tail);
            ParsedCommand::Search {
                cmd: shlex_join(main_cmd),
                query,
                path,
            }
        }
        Some((head, tail)) if head == "find" => {
            // Basic find support: capture path and common name filter
            let (query, path) = parse_find_query_and_path(tail);
            ParsedCommand::Search {
                cmd: shlex_join(main_cmd),
                query,
                path,
            }
        }
        Some((head, tail)) if head == "grep" => {
            let args_no_connector = trim_at_connector(tail);
            let non_flags: Vec<&String> = args_no_connector
                .iter()
                .filter(|p| !p.starts_with('-'))
                .collect();
            // Do not shorten the query: grep patterns may legitimately contain slashes
            // and should be preserved verbatim. Only paths should be shortened.
            let query = non_flags.first().cloned().map(|s| s.to_string());
            let path = non_flags.get(1).map(|s| short_display_path(s));
            ParsedCommand::Search {
                cmd: shlex_join(main_cmd),
                query,
                path,
            }
        }
        Some((head, tail)) if head == "cat" => {
            // Support both `cat <file>` and `cat -- <file>` forms.
            let effective_tail: &[String] = if tail.first().map(|s| s.as_str()) == Some("--") {
                &tail[1..]
            } else {
                tail
            };
            if effective_tail.len() == 1 {
                let name = short_display_path(&effective_tail[0]);
                ParsedCommand::Read {
                    cmd: shlex_join(main_cmd),
                    name,
                }
            } else {
                ParsedCommand::Unknown {
                    cmd: shlex_join(main_cmd),
                }
            }
        }
        Some((head, tail)) if head == "head" => {
            // Support `head -n 50 file` and `head -n50 file` forms.
            let has_valid_n = match tail.split_first() {
                Some((first, rest)) if first == "-n" => rest
                    .first()
                    .is_some_and(|n| n.chars().all(|c| c.is_ascii_digit())),
                Some((first, _)) if first.starts_with("-n") => {
                    first[2..].chars().all(|c| c.is_ascii_digit())
                }
                _ => false,
            };
            if has_valid_n {
                // Build candidates skipping the numeric value consumed by `-n` when separated.
                let mut candidates: Vec<&String> = Vec::new();
                let mut i = 0;
                while i < tail.len() {
                    if i == 0 && tail[i] == "-n" && i + 1 < tail.len() {
                        let n = &tail[i + 1];
                        if n.chars().all(|c| c.is_ascii_digit()) {
                            i += 2;
                            continue;
                        }
                    }
                    candidates.push(&tail[i]);
                    i += 1;
                }
                if let Some(p) = candidates.into_iter().find(|p| !p.starts_with('-')) {
                    let name = short_display_path(p);
                    return ParsedCommand::Read {
                        cmd: shlex_join(main_cmd),
                        name,
                    };
                }
            }
            ParsedCommand::Unknown {
                cmd: shlex_join(main_cmd),
            }
        }
        Some((head, tail)) if head == "tail" => {
            // Support `tail -n +10 file` and `tail -n+10 file` forms.
            let has_valid_n = match tail.split_first() {
                Some((first, rest)) if first == "-n" => rest.first().is_some_and(|n| {
                    let s = n.strip_prefix('+').unwrap_or(n);
                    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
                }),
                Some((first, _)) if first.starts_with("-n") => {
                    let v = &first[2..];
                    let s = v.strip_prefix('+').unwrap_or(v);
                    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
                }
                _ => false,
            };
            if has_valid_n {
                // Build candidates skipping the numeric value consumed by `-n` when separated.
                let mut candidates: Vec<&String> = Vec::new();
                let mut i = 0;
                while i < tail.len() {
                    if i == 0 && tail[i] == "-n" && i + 1 < tail.len() {
                        let n = &tail[i + 1];
                        let s = n.strip_prefix('+').unwrap_or(n);
                        if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) {
                            i += 2;
                            continue;
                        }
                    }
                    candidates.push(&tail[i]);
                    i += 1;
                }
                if let Some(p) = candidates.into_iter().find(|p| !p.starts_with('-')) {
                    let name = short_display_path(p);
                    return ParsedCommand::Read {
                        cmd: shlex_join(main_cmd),
                        name,
                    };
                }
            }
            ParsedCommand::Unknown {
                cmd: shlex_join(main_cmd),
            }
        }
        Some((head, tail)) if head == "nl" => {
            // Avoid treating option values as paths (e.g., nl -s "  ").
            let candidates = skip_flag_values(tail, &["-s", "-w", "-v", "-i", "-b"]);
            if let Some(p) = candidates.into_iter().find(|p| !p.starts_with('-')) {
                let name = short_display_path(p);
                ParsedCommand::Read {
                    cmd: shlex_join(main_cmd),
                    name,
                }
            } else {
                ParsedCommand::Unknown {
                    cmd: shlex_join(main_cmd),
                }
            }
        }
        Some((head, tail))
            if head == "sed"
                && tail.len() >= 3
                && tail[0] == "-n"
                && is_valid_sed_n_arg(tail.get(1).map(|s| s.as_str())) =>
        {
            if let Some(path) = tail.get(2) {
                let name = short_display_path(path);
                ParsedCommand::Read {
                    cmd: shlex_join(main_cmd),
                    name,
                }
            } else {
                ParsedCommand::Unknown {
                    cmd: shlex_join(main_cmd),
                }
            }
        }
        // Other commands
        _ => ParsedCommand::Unknown {
            cmd: shlex_join(main_cmd),
        },
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
/// Tests are at the top to encourage using TDD + Codex to fix the implementation.
mod tests {
    use super::*;

    fn shlex_split_safe(s: &str) -> Vec<String> {
        shlex_split(s).unwrap_or_else(|| s.split_whitespace().map(|s| s.to_string()).collect())
    }

    fn vec_str(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    fn assert_parsed(args: &[String], expected: Vec<ParsedCommand>) {
        let out = parse_command(args);
        assert_eq!(out, expected);
    }

    #[test]
    fn git_status_is_unknown() {
        assert_parsed(
            &vec_str(&["git", "status"]),
            vec![ParsedCommand::Unknown {
                cmd: "git status".to_string(),
            }],
        );
    }
}