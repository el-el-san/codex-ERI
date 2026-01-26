<h1 align="center">Codex CLI (ERI fork)</h1>

## このリポジトリについて
- OpenAI Rust 版 Codex CLI v0.91.0 をベースにしたクロスプラットフォームフォーク（最終同期: 2026-01-25 の upstream rust-v0.91.0）。
- Termux / WSL / SSH / コンテナでもビルド・ログイン・MCP が破綻しないよう、必要最小限のパッチだけを維持します。
- 主要ソースは `codex-rs/` 配下の Cargo ワークスペースで、Rust 1.90（`rust-toolchain.toml`）を前提としています。

## 上流からの主な差分
- TLS を自己完結化: `core` / `login` / `ollama` の `reqwest` に `native-tls-vendored` を付与し、Android/Termux などでのビルド成功率を改善。
- ログイン URL の起動を共通化 (`codex_core::util::open_url`):
  - Termux: `termux-open-url`
  - WSL: `cmd.exe /c start` → 失敗時に `wslview`
  - SSH/コンテナ: 自動起動を抑止し、手動で開く案内を表示
  - Linux デスクトップ / macOS / Windows: 各 OS の標準コマンドとフォールバック（`BROWSER` / `xdg-open` / `gio open` / `sensible-browser` / Firefox/Chrome 系）
- MCP サーバー起動時に Termux/Android の環境変数を保持（`rmcp-client` の `DEFAULT_ENV_VARS` を拡張）。
- 詳細手順や再適用チェックリストは `docs/01-cross-platform-update-guide.md` を参照。上流確認手順は `docs/00-update-chk.md` にあります。

## リポジトリ構成
- `codex-rs/` … Rust ワークスペース（`codex` マルチツール、`codex-exec`、`codex-tui` ほか）
- `docs/` … 差分ガイドと更新チェックリスト
- `downloads/` … 参照用に取得した upstream アーカイブやビルド済みバイナリ（必ずしも最新ではありません）

## ビルド & インストール
前提: Rust 1.90（`rust-toolchain.toml` で固定）。TLS ライブラリは同梱するため追加の依存は不要です。

```bash
cd codex-rs
cargo build --release -p codex-cli -p codex-exec -p codex-tui
```

生成される主なバイナリ
- `codex-rs/target/release/codex` … マルチツール（対話 TUI + サブコマンド）
- `codex-rs/target/release/codex-exec` … 非対話実行専用
- `codex-rs/target/release/codex-tui` … TUI 単体起動

## 基本的な使い方
```bash
codex "これをやって"                   # 対話 TUI を起動（プロンプト省略可）
codex resume --last                   # 直近の対話セッションを再開
codex exec "これをやって"             # 非対話で1タスク実行
codex exec resume --last "続き"       # 非対話セッションを再開して追加指示
codex login                           # ブラウザでログイン（環境により手動案内）
codex logout                          # 保存した認証情報を削除
codex mcp list                        # MCP サーバー設定の一覧
codex mcp add tools -- ./server.sh    # stdio MCP サーバーを追加
codex mcp add remote --url https://example --bearer-token-env-var TOKEN
codex mcp-server                      # Codex を MCP サーバーとして起動 (stdio)
codex sandbox linux --full-auto -- echo hello   # Landlock+seccomp でコマンド実験
codex apply                           # 直近の diff を git apply 相当で適用
```

## よく使うオプション
- `--sandbox {read-only|workspace-write|danger-full-access}` / `--full-auto` / `--dangerously-bypass-approvals-and-sandbox`
- `--oss --local-provider {ollama|lmstudio}`（ローカル LLM 利用） / `--search`（TUI で web_search ツールを有効化）
- `--image path1,path2`（初回プロンプトに画像を添付） / `--profile name`（`config.toml` のプロファイル切替）
- `--output-schema schema.json`（非対話 `codex exec` で最終レスポンスの JSON 形を指定）

## 設定
- 既定パスは `~/.codex/config.toml`（`CODEX_HOME` で上書き可）。スキーマは upstream v0.91.0 と同じで、MCP の環境変数/HTTP ヘッダー設定も同スキーマです（環境変数の保持は `rmcp-client` 側で実装）。
- ログは `~/.codex/log/` に保存されます。

## 環境別のログイン挙動
- Termux: `termux-open-url` を使用（`pkg install termux-api` と Termux:API アプリが必要）。失敗時は URL を手動で開いてください。
- WSL: `cmd.exe /c start` → 失敗時 `wslview`。どちらも失敗した場合は URL を手動で開きます。
- SSH/コンテナ: 自動起動を抑止し、URL と案内のみ表示します。
- Linux/macOS/Windows: OS 標準のコマンドと主要ブラウザへフォールバック。

## 既知の注意点
- `native-tls-vendored` によりビルド時間とバイナリサイズがわずかに増えます。
- WSL/SSH/コンテナではブラウザ自動起動を抑止しているため、表示された URL を自分で開く必要があります。

## ライセンス
- オリジナル著作権: 2025 OpenAI (Apache License 2.0)
- フォーク改変: 2025 ERI（同ライセンス。詳細は `LICENSE` / `NOTICE` を参照）
