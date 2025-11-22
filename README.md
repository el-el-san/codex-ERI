<h1 align="center">Codex CLI (ERI fork)</h1>

## このリポジトリについて
- OpenAI の Rust 版 Codex CLI v0.63.0 をベースにしたフォークです。
- Termux/WSL/SSH/コンテナを含む多様な環境で素直に動くよう、クロスプラットフォームのパッチを維持することが目的です。
- 主要ソースは `codex-rs/` 配下の Cargo ワークスペースで、Rust 1.90 でビルドします。

## 現状の差分（上流との違い）
- TLS を自己完結化: `core` / `login` / `ollama` の `reqwest` に `native-tls-vendored` を付与し、Android/Termux などでのビルド成功率を改善。
- ログイン URL 起動ロジックの共通化 (`codex_core::util::open_url`):
  - Termux: `termux-open-url`
  - WSL: `cmd.exe /c start` → 失敗時は `wslview`
  - SSH/コンテナ: 自動起動を抑止し手動案内を表示
  - Linux デスクトップ / macOS / Windows: 各 OS の標準コマンドを利用
- MCP サーバー起動時に Termux/Android 系の環境変数を保持（`rmcp-client` の `DEFAULT_ENV_VARS` を拡張）。
- それ以外の機能は概ね upstream v0.63.0 と同等です。以前の README にあったカスタムスラッシュコマンドや独自 allowlist といった記述は未実装のため削除しました。

差分の詳細: `docs/01-cross-platform-update-guide.md`  
アップデート手順: `docs/00-update-chk.md`

## リポジトリ構成
- `codex-rs/` … Rust ワークスペース（`cli` / `exec` / `tui` ほか）
- `docs/` … 差分ガイドと更新チェックリスト
- `downloads/` … ビルド済みバイナリの保存用フォルダ（常に最新とは限りません）

## ビルド & インストール
前提: Rust 1.90（`rust-toolchain.toml` で固定）。TLS ライブラリは同梱するため追加の依存は不要です。

```bash
cd codex-rs
cargo build --release -p cli -p exec -p tui
```

生成される主なバイナリ
- `target/release/codex` … マルチツール（対話 TUI + サブコマンド）
- `target/release/codex-exec` … 非対話実行専用
- `target/release/codex-tui` … TUI 単体起動

## 使い方
```bash
codex                                 # 対話 TUI を起動
codex exec "これをやって"             # 非対話で1タスク実行
codex exec resume --last "続き"       # 直近セッションを再開して追加指示
codex login                           # ブラウザでログイン（環境により手動案内あり）
codex logout                          # 認証情報を削除
codex mcp list                        # MCP サーバー設定の一覧
codex mcp add tools --command ./server.sh   # MCP サーバーを追加
codex mcp-server                      # Codex を MCP サーバーとして起動 (stdio)
codex sandbox linux -- echo hello     # Landlock+seccomp 下でコマンド実験
codex apply                           # 直近の diff を git apply 相当で適用
```

よく使うオプション
- `--sandbox {read-only|workspace-write|danger-full-access}` / `--full-auto` / `--dangerously-bypass-approvals-and-sandbox`
- `--oss --local-provider {ollama|lmstudio}` でローカル LLM を利用
- `--image path1,path2` で初回プロンプトに画像を添付
- `--output-schema schema.json` で最終レスポンスの JSON 形式を指定
- `--profile name` で `config.toml` のプロファイルを切り替え

## 設定
- 既定の設定パスは `~/.codex/config.toml`（`CODEX_HOME` で上書き可能）。
- スキーマは upstream v0.63.0 と同じです。MCP の環境変数/HTTP ヘッダー設定も同スキーマで、追加の環境変数保持は `rmcp-client` 側で行います。
- ログは `~/.codex/log/` に保存されます。

## 既知の注意点
- Termux でログイン URL を開くには `pkg install termux-api` と Termux:API アプリが必要です。失敗時は表示された URL を手動で開いてください。
- WSL/SSH/コンテナではブラウザ自動起動を抑止するため、案内に従って URL を手動で開く必要があります。
- `native-tls-vendored` を有効化しているため、ビルド時間とバイナリサイズがわずかに増えます。

## ライセンス
- オリジナル著作権: 2025 OpenAI (Apache License 2.0)
- フォーク改変: 2025 ERI（同ライセンス。詳細は `LICENSE` / `NOTICE` を参照）
