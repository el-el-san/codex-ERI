# クロスプラットフォーム対応・差分調査ガイド

このドキュメントは、`codex-rs` のクロスプラットフォーム（Linux/macOS/Windows/WSL/Android-Termux/SSH/Container）対応のために行った変更点と、
今後上流が更新された際に同等の対応を再適用する手順をまとめたものです。


## 1. 実装済みの変更点

### 1.1 依存関係（TLS）
`reqwest` に `native-tls-vendored` を付与（TLS を自己完結化）
- 変更ファイル:
  - `core/Cargo.toml`
  - `ollama/Cargo.toml`
  - `login/Cargo.toml`
  - （他に `reqwest` を使うクレートが増えた場合は同様に付与）

### 1.2 ブラウザ起動（OS/環境別分岐）
- `core/src/util.rs` に `open_url` 関数を追加（Termux/WSL/SSH/Container/各OS を考慮）
- `login/src/server.rs` は `webbrowser` 依存を削除し、`codex_core::util::open_url` を使用
- 変更ファイル:
  - `core/src/util.rs`（新規関数 `open_url` と環境検知関数を追加）
  - `login/Cargo.toml`（`webbrowser` 削除）
  - `login/src/server.rs`（ブラウザ起動処理を `open_url` 呼び出しへ差し替え）

### 1.3 MCP環境変数保持（Termux対応）
- MCPサーバー起動時に`env_clear()`が呼ばれるが、Termuxで必要な環境変数が削除される問題を修正
- 変更ファイル:
  - `mcp-client/src/mcp_client.rs`（`DEFAULT_ENV_VARS`にTermux環境変数を追加）
- 追加した環境変数:
  - `TERMUX_VERSION`, `PREFIX`, `TERMUX_APK_RELEASE`, `TERMUX_APP_PID`
  - `ANDROID_ROOT`, `ANDROID_DATA`
  - `LD_LIBRARY_PATH`, `LD_PRELOAD`（動的リンクに必須）

## 2. 変更の意図と効果

- TLS 依存の安定化（`native-tls-vendored`）
  - 背景: Android/Termux や一部環境でシステムの OpenSSL が使えない・不整合が起きるケースがある
  - 対応: `reqwest` に `native-tls-vendored` を付与し、TLS 実装をバンドル（ビルド成功性と可搬性向上）

- ブラウザ起動の環境検知
  - Termux（Android）: `termux-open-url` を使用
    - 備考: `termux-open-url` は Termux:API アプリと `pkg install termux-api` が必要
  - WSL: `cmd.exe /c start` もしくは `wslview`
  - SSH/Container: 自動起動は抑止し、URL の手動オープンを指示（フロー継続を阻害しない扱いに）
  - OS 別: macOS `open`、Linux `xdg-open`→各ブラウザ、Windows `cmd /c start`
  - Linux では `BROWSER` 環境変数や `gio open`、`sensible-browser`、主要ブラウザ（Firefox/Chrome/Chromium）もフォールバックとして試行
  - 効果: 多様な実行環境での「ログイン用 URL を開く」挙動が破綻しない



## 3. 次回以降の再適用手順（上流更新を取り込む際のチェックリスト）

以下は、新しい上流から Rust 実装へ変更を取り込む際に、クロスプラットフォーム対応を保つための手順です。

### 3.1 `reqwest` への TLS 機能付与を再確認
- 対象クレート: `core` / `ollama` / `login`（他に `reqwest` を使うクレートが増えたら同様に）
- Cargo.toml 例: `reqwest = { version = "0.12", features = ["json", "stream", "native-tls-vendored"] }`
- 注意: `blocking` 機能を使う箇所（`login` 等）は `features = ["json", "blocking", "native-tls-vendored"]` のように併記

### 3.2 ブラウザ起動の共通ロジックを適用
- `core/src/util.rs` の `open_url` の存在を確認し、呼び出し側（例: `login/src/server.rs`）で使用する
- もし上流で `webbrowser` クレートに戻っていたら、以下の分岐ロジックへ差し替え:
  - Termux: `termux-open-url`
  - WSL: `cmd.exe /c start` → 失敗時 `wslview`
  - SSH/Container: 自動起動を回避し、URL を出力
  - macOS/Linux/Windows: それぞれ `open` / `xdg-open` / `cmd /c start`
- `login/src/server.rs` のリダイレクト開始箇所（認可 URL を開く処理）で上記関数を使用すること

### 3.3 MCP環境変数保持の確認
- `mcp-client/src/mcp_client.rs` の `DEFAULT_ENV_VARS` にTermux環境変数が含まれているか確認
- 必要な環境変数:
  - 基本: `TERMUX_VERSION`, `PREFIX`
  - Android関連: `ANDROID_ROOT`, `ANDROID_DATA`, `TERMUX_APK_RELEASE`, `TERMUX_APP_PID`
  - 動的リンク: `LD_LIBRARY_PATH`, `LD_PRELOAD`

### 3.4 ふるまいの違い（失敗時の扱い）を揃える
- `login` のブラウザ起動は `OpenUrlStatus::Suppressed` をハンドリングし、警告＋URL表示でフローを継続する
- `core::util::open_url` は実行不能なケースのみ `Err` を返し、環境上の制約は `OpenUrlStatus::Suppressed` で伝達されるため、呼び出し側はメッセージ表示などを行う

### 3.5 最小限の動作確認
- Linux（X11/Wayland）: `xdg-open` が動作すること
- macOS: `open` で URL が開くこと
- Windows: `cmd /c start` が使えること
- WSL: `cmd.exe /c start` または `wslview` のいずれかで開けること
- Termux（Android）: `termux-open-url` で URL が開くこと、MCP機能が正常動作すること
- SSH/Container: 自動オープンは抑止され、URL が出力されること

## 4. 実用的な差分確認コマンド

上流と現状の差分が多い場合でも、まず「どのファイルが変更されたか」を把握するのが有効です。

```sh
# 例: 変更の多い領域を重点的に確認
diff -u upstream/codex-rs/core/src/util.rs codex-rs/core/src/util.rs
diff -u upstream/codex-rs/login/src/server.rs codex-rs/login/src/server.rs
diff -u upstream/codex-rs/tui/Cargo.toml codex-rs/tui/Cargo.toml
diff -u upstream/codex-rs/tui/src/clipboard_paste.rs codex-rs/tui/src/clipboard_paste.rs
diff -u upstream/codex-rs/mcp-client/src/mcp_client.rs codex-rs/mcp-client/src/mcp_client.rs
```

## 5. 参考：主な変更の実装例

### 5.1 `Cargo.toml`の例
```toml
# reqwestの設定
reqwest = { version = "0.12", features = ["json", "stream", "native-tls-vendored"] }

# loginクレートでは
reqwest = { version = "0.12", features = ["json", "blocking", "native-tls-vendored"] }

# tuiクレートでは（Android対応）
[target.'cfg(not(target_os = "android"))'.dependencies]
arboard = "3"
```

### 5.2 ブラウザ起動の実装（core/src/util.rs の open_url 関数）
```rust
pub fn open_url(url: &str) -> Result<OpenUrlStatus, OpenUrlError> {
    if url.is_empty() {
        return Ok(OpenUrlStatus::Suppressed {
            reason: "No URL provided".into(),
        });
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        // Termux: termux-open-url
        // WSL: cmd.exe /c start → 失敗時 wslview
        // SSH/Container: OpenUrlStatus::Suppressed で手動案内
        // Linuxデスクトップ: BROWSER, xdg-open, gio open, sensible-browser, Firefox/Chrome/Chromium を順に試行
    }

    #[cfg(target_os = "macos")]
    {
        // open コマンド
    }

    #[cfg(target_os = "windows")]
    {
        // cmd /C start
    }
}
```

注: `login/src/server.rs` は `OpenUrlStatus::Suppressed` を受け取り、URL を表示してフローを継続する

### 5.3 MCP環境変数の保持
```rust
// mcp-client/src/mcp_client.rs
const DEFAULT_ENV_VARS: &[&str] = &[
    // ... 既存の環境変数 ...
    
    // Termux-specific
    "TERMUX_VERSION",
    "PREFIX",
    "TERMUX_APK_RELEASE",
    "TERMUX_APP_PID",
    "ANDROID_ROOT",
    "ANDROID_DATA",
    "LD_LIBRARY_PATH",
    "LD_PRELOAD",
    // ... その他必要な環境変数 ...
];
```

## 6. GitHub Actions CI/CD での注意点

クロスプラットフォームビルドのCI/CD設定で以下の点に注意が必要：

### 6.1 Windows環境でのキャッシュクリア
- UnixコマンドとWindowsコマンドを分ける必要がある
- Unix: `rm -rf ~/.cargo/registry/cache`
- Windows: `rmdir /s /q "%USERPROFILE%\.cargo\registry\cache"`

### 6.2 Windows ARM64ビルド
- Rustターゲットの明示的な追加が必要: `rustup target add aarch64-pc-windows-msvc`

### 6.3 テストファイルとGitignore
- `.gitignore`の過度に広範なパターン（例：`test*`）に注意
- テストファイルやfixtureファイルがGitに追跡されているか確認が必要

## 7. 最近の更新履歴

### 2025-09-06 更新内容
- （ガイド初版）クロスプラットフォーム対応の方針を整理

### 2025-09-11 更新内容
- **reqwest依存関係**: `core`、`login`、`ollama` の各 `Cargo.toml` に `native-tls-vendored` を追加
- **ブラウザ起動処理**:
  - `core/src/util.rs` に `open_url` と環境検知（`is_termux`/`is_wsl`/`is_ssh`/`is_container`）を実装
  - `login/src/server.rs` は `webbrowser` 依存を削除し `open_url` を使用
- **MCP環境変数**: `mcp-client/src/mcp_client.rs` の `DEFAULT_ENV_VARS` に Termux/Android 関連を追加

## 8. トラブルシューティング

### 8.1 Termux環境でMCP機能が動作しない
- 症状: MCPサーバーが起動するが、コマンド実行でエラーが発生
- 原因: `env_clear()`により必要な環境変数が削除される
- 対処: `mcp-client/src/mcp_client.rs`の`DEFAULT_ENV_VARS`を確認

### 8.2 ブラウザが自動で開かない
- Termux: `termux-open-url`コマンドがインストールされているか確認（`pkg install termux-api`）
- WSL/SSH/Container: 手動でURLを開く必要がある（設計通りの動作）

### 8.3 TLSエラーが発生
- 症状: HTTPSリクエストでTLS/SSL関連のエラー
- 対処: `reqwest`の`native-tls-vendored`機能が有効になっているか確認

---

このガイドに沿って差分を適用すれば、上流更新のたびに同様のクロスプラットフォーム対応を素早く再現できます。追加で対応が必要になった環境が出てきた場合は、本ドキュメントに追記してください。

### 2025-09-18 更新内容
- `core/src/util.rs` の `open_url` が `OpenUrlStatus` / `OpenUrlError` を返すようになり、環境制約時は `Suppressed` で通知
- Linux系では `BROWSER`・`gio open`・`sensible-browser`・Firefox/Chrome/Chromium を順にフォールバック
- `login/src/server.rs` は `OpenUrlStatus::Suppressed` を受けて自動で案内メッセージと URL を表示
- `mcp-client/src/mcp_client.rs` の `DEFAULT_ENV_VARS` に Android / Termux 向けの詳細な環境変数を明示
