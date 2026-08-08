# クロスプラットフォーム対応・差分調査ガイド

このドキュメントは、`codex-rs` のクロスプラットフォーム（Linux/macOS/Windows/WSL/Android-Termux/SSH/Container）対応のために行った変更点と、
今後上流が更新された際に同等の対応を再適用する手順をまとめたものです。


## 1. 実装済みの変更点

### 1.1 依存関係（TLS）
`reqwest` に `native-tls-vendored` を付与（TLS を自己完結化）
- 変更ファイル:
  - `http-client/Cargo.toml`
  - （上流で HTTP クライアントの配置が変わった場合は、Android 向け依存グラフで有効になる `reqwest` に同様に付与）
- upstream 0.147.0 では HTTP 通信が `http-client` に集約され、従来の `core` / `ollama` / `login` は `reqwest` を直接依存しない

- `login/src/server.rs` に `open_url` 相当のローカル実装を追加（Termux/WSL/SSH/Container/各OS を考慮）
- MCP の OAuth ログイン（`rmcp-client/src/perform_oauth_login.rs`）も `webbrowser` を使わず、`rmcp-client/src/utils.rs` の同等ロジックでブラウザ起動を試みる
  - 注: 現行上流では `login` と `core` の依存関係上、`login` から `codex-core` に直接依存できないため、`login` 側もローカル実装として保持する
- 変更ファイル:
  - `login/Cargo.toml`（`webbrowser` 削除、`shlex` 追加）
  - `login/src/server.rs`（ブラウザ起動処理と環境検知をローカル実装へ差し替え）
  - `rmcp-client/src/utils.rs`（`open_url` と環境検知関数を追加）
  - `rmcp-client/src/perform_oauth_login.rs`（ブラウザ起動処理を `open_url` 呼び出しへ差し替え）
  - `rmcp-client/Cargo.toml`（`webbrowser` 削除）

### 1.3 MCP環境変数保持（Termux対応）
- MCPサーバー起動時に`env_clear()`が呼ばれるが、Termuxで必要な環境変数が削除される問題を修正
- 変更ファイル:
  - `rmcp-client/src/utils.rs`（`DEFAULT_ENV_VARS`にTermux環境変数を追加）
- 追加した環境変数:
  - `TERMUX_VERSION`, `PREFIX`, `TERMUX_APK_RELEASE`, `TERMUX_APP_PID`
  - `ANDROID_ROOT`, `ANDROID_DATA`
  - `LD_LIBRARY_PATH`, `LD_PRELOAD`（動的リンクに必須）

### 1.4 Androidビルド警告の抑止（clipboard_paste）
- Androidターゲットで `tui/src/clipboard_paste.rs` の unused import / dead_code 警告が出るため、条件付きコンパイルで抑止
- 変更内容:
  - `tempfile::Builder` の import を `#[cfg(not(target_os = "android"))]` で限定
  - `PasteImageError` / Android版 `paste_image_as_png` に `#[cfg_attr(target_os = "android", allow(dead_code))]` を付与
- 変更ファイル:
  - `tui/src/clipboard_paste.rs`

### 1.5 Androidでの code mode / V8 分離
- upstream 0.147.0 では V8 ランタイムが `code-mode-runtime`、プロセス境界が `code-mode-host` へ分離された
- Android 用の `codex-cli` / `codex-exec` / `codex-tui` はこれらを依存グラフに含まず、外部ホストが利用できない場合は upstream のフォールバックで code mode を無効化する
- 0.145.0 以前に必要だった `core` / `tools` の Android 用スタブと依存の `cfg` 分岐は削除し、0.147.0 の upstream 実装へ戻した
- **ただし `effective_tool_mode` の Android 強制 Direct ガードは復元が必要だった**（§3.9 参照）
  - upstream の Direct フォールバックは `CodeMode` のみで、`CodeModeOnly` は意図的に fail-closed
  - gpt-5.6 系はサーバー側モデルメタデータで `tool_mode: code_mode_only` が指定されており、
    ガードが無いと Android では直接ツールが一切公開されず実使用不可になる
- 更新時は `cargo tree --target aarch64-linux-android` で Android 用パッケージから `v8` / `rusty_v8` が到達不能であることを確認する

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
- upstream 0.147.0 の対象クレートは `http-client`
- `http-client/Cargo.toml` の `reqwest` features に `native-tls-vendored` が含まれることを確認
- 上流更新で依存配置が変わった場合は、`cargo tree --target aarch64-linux-android -i openssl-src -e features` で `openssl-sys` が vendored OpenSSL を使うことを確認
- 上流から削除された `core` / `ollama` / `login` の直接 `reqwest` 依存は復元しない

### 3.2 ブラウザ起動の共通ロジックを適用
- もし上流で `webbrowser` クレートに戻っていたら、`login/src/server.rs` と `rmcp-client/src/utils.rs` / `perform_oauth_login.rs` を以下の分岐ロジックへ差し替え:
  - Termux: `termux-open-url`
  - WSL: `cmd.exe /c start` → 失敗時 `wslview`
  - SSH/Container: 自動起動を回避し、URL を出力
  - macOS/Linux/Windows: それぞれ `open` / `xdg-open` / `cmd /c start`
- `login/src/server.rs` のリダイレクト開始箇所（認可 URL を開く処理）で上記関数を使用すること
- MCP の OAuth ログイン（`rmcp-client/src/perform_oauth_login.rs`）も同様に適用すること

### 3.3 MCP環境変数保持の確認
- `rmcp-client/src/utils.rs` の `DEFAULT_ENV_VARS` にTermux環境変数が含まれているか確認
  - **注**: 過去のバージョンでは `mcp-client/src/mcp_client.rs` にありましたが、現在は `rmcp-client/src/utils.rs` に統一されています
- 必要な環境変数:
  - 基本: `TERMUX_VERSION`, `PREFIX`
  - Android関連: `ANDROID_ROOT`, `ANDROID_DATA`, `TERMUX_APK_RELEASE`, `TERMUX_APP_PID`
  - 動的リンク: `LD_LIBRARY_PATH`, `LD_PRELOAD`

### 3.4 ふるまいの違い（失敗時の扱い）を揃える
- `login` / `rmcp-client` のブラウザ起動は `OpenUrlStatus::Suppressed` をハンドリングし、警告＋URL表示でフローを継続する
- `open_url` 実装は実行不能なケースのみ `Err` を返し、環境上の制約は `OpenUrlStatus::Suppressed` で伝達する

### 3.5 最小限の動作確認
- Linux（X11/Wayland）: `xdg-open` が動作すること
- macOS: `open` で URL が開くこと
- Windows: `cmd /c start` が使えること
- WSL: `cmd.exe /c start` または `wslview` のいずれかで開けること
- Termux（Android）: `termux-open-url` で URL が開くこと、MCP機能が正常動作すること
- SSH/Container: 自動オープンは抑止され、URL が出力されること

### 3.6 Androidでのファイルロック非対応（arg0 / installation_id）
- `arg0/src/lib.rs` で `File::try_lock()` がAndroidのファイルシステムで非対応のためエラーになる
- `prepend_path_entry_for_codex_aliases` 内と `try_lock_dir` 内の `try_lock()` 呼び出しを `#[cfg(not(target_os = "android"))]` で除外
- Android版では `try_lock_dir` はロック確認なしで常に `Some(lock_file)` を返す
- `core/src/installation_id.rs` で `file.lock()` (Rust std の `File::lock`) がAndroidで `lock() not supported` エラーになる
  - `file.lock()?;` を `#[cfg(not(target_os = "android"))]` で除外
  - これにより `thread/start failed during TUI bootstrap` エラーが解消される

- `thread-store/src/local/writer_lock.rs`（0.147.0 で新設）の `file.lock()` / `file.try_lock()` も同様にAndroidで非対応
  - `lock_coordination` 内の `file.lock()` と `acquire` 内の `file.try_lock()` を `#[cfg(not(target_os = "android"))]` で除外
  - Android では `remove_stale_thread_locks` は stale 判定ができないため no-op とする
  - 未対策だと `thread/start failed during TUI bootstrap: ... .coordination.lock: lock() not supported` で起動不可になる

### 3.7 Androidビルドでの警告抑止（clipboard_paste）
- `tui/src/clipboard_paste.rs` で Androidビルド時に `unused import` / `dead_code` が出る場合は以下を再適用
  - `tempfile::Builder` の import を `#[cfg(not(target_os = "android"))]` で限定
  - `PasteImageError` と Android版 `paste_image_as_png` に `#[cfg_attr(target_os = "android", allow(dead_code))]` を付与

### 3.8 Androidクロスビルドで `rusty_v8` 404 が出る場合
- まず `cargo tree --target aarch64-linux-android -p codex-cli -p codex-exec -p codex-tui` を確認する
- upstream 0.147.0 の正常な構成では Android 用 3 パッケージから `v8` / `rusty_v8` へ到達しない
- 到達する場合は `code-mode-runtime` / `code-mode-host` が Android パッケージへ混入した依存経路を特定し、その依存をホスト専用へ戻す
- 0.145.0 以前の Android 用スタブを無条件に再適用せず、現行 upstream の外部ホスト分離を優先する

### 3.9 Android で code mode 系ツールモードを Direct に強制する
- `core/src/tools/mod.rs` の `effective_tool_mode` 先頭に以下のガードを適用する
  ```rust
  #[cfg(target_os = "android")]
  {
      let _ = turn_context;
      return ToolMode::Direct;
  }
  ```
- 背景: gpt-5.6 系はサーバー側モデルメタデータが `tool_mode: code_mode_only` を返し、
  feature フラグより優先される（設定での無効化は不可）
- upstream はホスト不在時の Direct フォールバックを `CodeMode` にのみ用意し、
  `CodeModeOnly` は意図的に fail-closed（直接ツールを公開しない）
- Android は `codex-code-mode-host`（V8 依存）を提供できないため、ガードが無いと
  会話はできてもファイル操作・コマンド実行が一切できない状態になる
- 0.145.0 にも同趣旨のガードがあったが、0.147.0 同期時にスタブ削除で失われたため復元した
- `core/src/tools/code_mode/mod.rs` の `take_unavailable_warning` で、
  Android かつ実効モードが `ToolMode::Direct` の場合のみ警告を返さない
  - Android では Direct への切り替えが常に意図した挙動であり、
    `Code Mode is unavailable ... Falling back to direct tools` を毎回表示する必要がないため
  - Direct ツールの公開には影響せず、Android 以外の利用不可警告と、
    Android でも fail-closed になった場合の重要な警告は維持される

## 4. 実用的な差分確認コマンド

上流と現状の差分が多い場合でも、まず「どのファイルが変更されたか」を把握するのが有効です。

```sh
# 例: 変更の多い領域を重点的に確認
diff -u upstream/codex-rs/core/src/util.rs codex-rs/core/src/util.rs
diff -u upstream/codex-rs/login/src/server.rs codex-rs/login/src/server.rs
diff -u upstream/codex-rs/tui/Cargo.toml codex-rs/tui/Cargo.toml
diff -u upstream/codex-rs/tui/src/clipboard_paste.rs codex-rs/tui/src/clipboard_paste.rs
diff -u upstream/codex-rs/rmcp-client/src/utils.rs codex-rs/rmcp-client/src/utils.rs
diff -u upstream/codex-rs/rmcp-client/src/perform_oauth_login.rs codex-rs/rmcp-client/src/perform_oauth_login.rs
```

## 5. 参考：主な変更の実装例

### 5.1 `Cargo.toml`の例
```toml
# http-client/Cargo.toml
reqwest = { workspace = true, features = [
    "json",
    "native-tls-vendored",
    "rustls-tls-native-roots",
    "stream",
] }

# tuiクレートでは（Android対応）
[target.'cfg(not(target_os = "android"))'.dependencies]
arboard = "3"
```

### 5.2 ブラウザ起動の実装（login/src/server.rs / rmcp-client/src/utils.rs の open_url 関数）
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

注: `login/src/server.rs` と `rmcp-client/src/perform_oauth_login.rs` は `OpenUrlStatus::Suppressed` を受け取り、URL を表示してフローを継続する

### 5.3 MCP環境変数の保持
```rust
// rmcp-client/src/utils.rs
#[cfg(unix)]
pub(crate) const DEFAULT_ENV_VARS: &[&str] = &[
    // ... 既存の環境変数 ...

    // Termux/Android-specific
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

### 6.4 Androidクロスビルド時のリンク時間対策（LTO）
- `codex-rs/Cargo.toml` の `[profile.release]` で `lto = "thin"` を維持する
- 背景: `lto = "fat"` だと Android 向けクロスビルドでリンク工程が長時間化し、CI で `exit code 143`（プロセス終了）を誘発する場合がある
- `lto = "thin"` / `codegen-units = 1` でも Android 向け release build が長時間化する場合があるため、
  GitHub Actions `Build Android` では `CARGO_PROFILE_RELEASE_LTO=false` と
  `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16` で上書きする
- 再適用時チェック:
  - `codex-rs/Cargo.toml` の `[profile.release]` が `lto = "thin"` になっているか
  - `.github/workflows/build-android.yml` の `Build` ステップで `CARGO_PROFILE_RELEASE_LTO=false` と
    `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16` を設定しているか
  - GitHub Actions `Build Android` で `Build` ステップが完走するか

## 7. 最近の更新履歴

### 2026-08-08 更新内容（rust-v0.147.0）
- 上流 `rust-v0.147.0` を取り込み、`codex-rs` を同期
- 再適用・確認した差分:
  - HTTP 通信の集約に追従し、`http-client` の `reqwest` に `native-tls-vendored` を付与
  - `login` / `rmcp-client` のブラウザ起動を `open_url` ベースに統一
  - `rmcp-client` の Termux/Android 環境変数保持
  - `arg0` の Android `try_lock()` 回避
  - `core/src/installation_id.rs` の Android `file.lock()` 回避
  - `tui` の Android 向け警告抑止
  - upstream の外部 code-mode ホスト分離に追従し、旧 Android 用スタブを削除
  - Android 用 `codex-cli` / `codex-exec` / `codex-tui` の依存グラフに V8 がないことを確認
  - `codex-rs/Cargo.toml` の `[profile.release]` を `lto = "thin"` に維持
  - Android CI の `CARGO_PROFILE_RELEASE_LTO=false` /
    `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16` を維持
  - `quinn-proto` / `event-listener` / `memmap2` を監査済み修正版へ更新し、
    `serial_test` が `scc` 2 系を要求する `RUSTSEC-2026-0205` は理由を明記して一時除外
- GitHub Actions の Android aarch64 release build、Cargo audit、CodeQL が成功
- リリース後に発覚した不具合の修正（hotfix）:
  - `thread-store/src/local/writer_lock.rs`（0.147.0 新設）の `file.lock()` / `file.try_lock()` が
    Android で `lock() not supported` となり `thread/start failed during TUI bootstrap` で起動不可だったため、
    `arg0` / `installation_id` と同じ `#[cfg(not(target_os = "android"))]` パターンで回避
    （Android では stale クリーンアップも no-op）
  - gpt-5.6 系の `tool_mode: code_mode_only` が Android で fail-closed となり
    直接ツールが一切使えなかったため、`core/src/tools/mod.rs` の `effective_tool_mode` に
    Android では常に `ToolMode::Direct` を返すガードを復元（0.145.0 同等の対策、§3.9 参照）

### 2026-07-24 更新内容（rust-v0.145.0）
- 上流 `rust-v0.145.0` を取り込み、`codex-rs` を同期
- 再適用した差分:
  - `core/login/ollama` の `reqwest` に `native-tls-vendored`
  - `login` / `rmcp-client` のブラウザ起動を `open_url` ベースに統一
  - `rmcp-client` の Termux/Android 環境変数保持
  - Android では `code-mode` 依存と `exec` / `wait` 公開を無効化
  - 上流の `core/src/tools/code_mode/` モジュール分割に Android 用スタブを追従
  - `rollout-trace` の Android `codex-code-mode` 依存を無効化
  - `arg0` の Android `try_lock()` 回避
  - `core/src/installation_id.rs` の Android `file.lock()` 回避
  - `tui` の Android 向け警告抑止
  - `codex-rs/Cargo.toml` の `[profile.release]` を `lto = "thin"` に維持
  - Android CI の release build 時間対策として `CARGO_PROFILE_RELEASE_LTO=false` と
    `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16` を維持

### 2026-07-10 更新内容（rust-v0.144.0）
- 上流 `rust-v0.144.0` を取り込み、`codex-rs` を同期
- 再適用した差分:
  - `core/login/ollama` の `reqwest` に `native-tls-vendored`
  - `login` / `rmcp-client` のブラウザ起動を `open_url` ベースに統一
  - `rmcp-client` の Termux/Android 環境変数保持
  - Android では `code-mode` 依存と `exec` / `wait` 公開を無効化
  - `rollout-trace` の Android `codex-code-mode` 依存を無効化
  - `arg0` の Android `try_lock()` 回避
  - `core/src/installation_id.rs` の Android `file.lock()` 回避
  - `tui` の Android 向け警告抑止
  - `codex-rs/Cargo.toml` の `[profile.release]` を `lto = "thin"` に維持
  - Android CI の release build 時間対策として `CARGO_PROFILE_RELEASE_LTO=false` と
    `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16` を維持

### 2026-07-08 更新内容（rust-v0.143.0）
- 上流 `rust-v0.143.0` を取り込み、`codex-rs` を同期
- 再適用した差分:
  - `core/login/ollama` の `reqwest` に `native-tls-vendored`
  - `login` / `rmcp-client` のブラウザ起動を `open_url` ベースに統一
  - `rmcp-client` の Termux/Android 環境変数保持
  - Android では `code-mode` 依存と `exec` / `wait` 公開を無効化
  - `rollout-trace` の Android `codex-code-mode` 依存を無効化
  - `arg0` の Android `try_lock()` 回避
  - `core/src/installation_id.rs` の Android `file.lock()` 回避
  - `tui` の Android 向け警告抑止
  - `codex-rs/Cargo.toml` の `[profile.release]` を `lto = "thin"` に維持
  - Android CI の release build 時間対策として `CARGO_PROFILE_RELEASE_LTO=false` と
    `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16` を維持

### 2026-06-22 更新内容（rust-v0.141.0）
- 上流 `rust-v0.141.0` を取り込み、`codex-rs` を同期
- 再適用した差分:
  - `core/login/ollama` の `reqwest` に `native-tls-vendored`
  - `login` / `rmcp-client` のブラウザ起動を `open_url` ベースに統一
  - `rmcp-client` の Termux/Android 環境変数保持
  - Android では `code-mode` 依存と `exec` / `wait` 公開を無効化
  - `rollout-trace` の Android `codex-code-mode` 依存を無効化
  - `arg0` の Android `try_lock()` 回避
  - `core/src/installation_id.rs` の Android `file.lock()` 回避
  - `tui` の Android 向け警告抑止
  - `codex-rs/Cargo.toml` の `[profile.release]` を `lto = "thin"` に維持
  - Android CI の release build 時間対策として `CARGO_PROFILE_RELEASE_LTO=false` と
    `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16` を維持
  - 上流 `0.141.0` の `DEFAULT_WAIT_YIELD_TIME_MS = 10_000` に Android スタブを追従

### 2026-06-11 更新内容（rust-v0.139.0）
- 上流 `rust-v0.139.0` を取り込み、`codex-rs` を同期
- 再適用した差分:
  - `core/login/ollama` の `reqwest` に `native-tls-vendored`
  - `login` / `rmcp-client` のブラウザ起動を `open_url` ベースに統一
  - `rmcp-client` の Termux/Android 環境変数保持
  - Android では `code-mode` 依存と `exec` / `wait` 公開を無効化
  - `rollout-trace` の Android `codex-code-mode` 依存を無効化
  - `arg0` の Android `try_lock()` 回避
  - `core/src/installation_id.rs` の Android `file.lock()` 回避
  - `tui` の Android 向け警告抑止
  - `codex-rs/Cargo.toml` の `[profile.release]` を `lto = "thin"` に維持
  - Android CI の release build 時間対策として `CARGO_PROFILE_RELEASE_LTO=false` と
    `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16` を維持

### 2026-05-31 更新内容（rust-v0.135.0）
- 上流 `rust-v0.135.0` を取り込み、`codex-rs` を同期
- 再適用した差分:
  - `core/login/ollama` の `reqwest` に `native-tls-vendored`
  - `login` / `rmcp-client` のブラウザ起動を `open_url` ベースに統一
  - `rmcp-client` の Termux/Android 環境変数保持
  - Android では `code-mode` 依存と `exec` / `wait` 公開を無効化
  - `rollout-trace` の Android `codex-code-mode` 依存を無効化
  - `arg0` の Android `try_lock()` 回避
  - `core/src/installation_id.rs` の Android `file.lock()` 回避
  - `tui` の Android 向け警告抑止
  - `codex-rs/Cargo.toml` の `[profile.release]` を `lto = "thin"` に維持
  - Android CI の release build 時間対策として `CARGO_PROFILE_RELEASE_LTO=false` と
    `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16` を維持

### 2026-05-22 更新内容（rust-v0.133.0）
- 上流 `rust-v0.133.0` を取り込み、`codex-rs` を同期
- 再適用した差分:
  - `core/login/ollama` の `reqwest` に `native-tls-vendored`
  - `login` / `rmcp-client` のブラウザ起動を `open_url` ベースに統一
  - `rmcp-client` の Termux/Android 環境変数保持
  - Android では `code-mode` 依存と `exec` / `wait` 公開を無効化
  - `rollout-trace` の Android `codex-code-mode` 依存を無効化
  - `arg0` の Android `try_lock()` 回避
  - `core/src/installation_id.rs` の Android `file.lock()` 回避
  - `tui` の Android 向け警告抑止
  - `codex-rs/Cargo.toml` の `[profile.release]` を `lto = "thin"` に維持
  - Android CI の release build 時間対策として `CARGO_PROFILE_RELEASE_LTO=false` と
    `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16` を追加

### 2026-05-09 更新内容（rust-v0.130.0）
- 上流 `rust-v0.130.0` を取り込み、`codex-rs` を同期
- 再適用した差分:
  - `core/login/ollama` の `reqwest` に `native-tls-vendored`
  - `login` / `rmcp-client` のブラウザ起動を `open_url` ベースに統一
  - `rmcp-client` の Termux/Android 環境変数保持
  - Android では `code-mode` 依存と `exec` / `wait` 公開を無効化
  - `rollout-trace` の Android `codex-code-mode` 依存を無効化
  - `arg0` の Android `try_lock()` 回避
  - `core/src/installation_id.rs` の Android `file.lock()` 回避
  - `tui` の Android 向け警告抑止
  - `codex-rs/Cargo.toml` の `[profile.release]` を `lto = "thin"` に維持

### 2026-05-09 更新内容（rust-v0.129.0）
- 上流 `rust-v0.129.0` を取り込み、`codex-rs` を同期
- 再適用した差分:
  - `core/login/ollama` の `reqwest` に `native-tls-vendored`
  - `login` / `rmcp-client` のブラウザ起動を `open_url` ベースに統一
  - `rmcp-client` の Termux/Android 環境変数保持
  - Android では `code-mode` 依存と `exec` / `wait` 公開を無効化
  - `rollout-trace` の Android `codex-code-mode` 依存を無効化
  - `arg0` の Android `try_lock()` 回避
  - `core/src/installation_id.rs` の Android `file.lock()` 回避
  - `tui` の Android 向け警告抑止
  - `codex-rs/Cargo.toml` の `[profile.release]` を `lto = "thin"` に維持

### 2026-05-01 更新内容
- 上流 `rust-v0.128.0` を取り込み、`codex-rs` を同期
- 再適用した差分:
  - `core/login/ollama` の `reqwest` に `native-tls-vendored`
  - `login` / `rmcp-client` のブラウザ起動を `open_url` ベースに統一
  - `rmcp-client` の Termux/Android 環境変数保持
  - Android では `code-mode` 依存と `exec` / `wait` 公開を無効化
  - `arg0` の Android `try_lock()` 回避
  - `core/src/installation_id.rs` の Android `file.lock()` 回避
  - `tui` の Android 向け警告抑止
  - `codex-rs/Cargo.toml` の `[profile.release]` を `lto = "thin"` に維持

### 2026-04-24 更新内容
- 上流 `rust-v0.124.0` を取り込み、`codex-rs` を同期
- 再適用した差分:
  - `core/login/ollama` の `reqwest` に `native-tls-vendored`
  - `login` / `rmcp-client` のブラウザ起動を `open_url` ベースに統一
  - `rmcp-client` の Termux/Android 環境変数保持
  - Android では `code-mode` 依存と `exec` / `wait` 公開を無効化
  - `arg0` の Android `try_lock()` 回避
  - `core/src/installation_id.rs` の Android `file.lock()` 回避
  - `tui` の Android 向け警告抑止
  - `codex-rs/Cargo.toml` の `[profile.release]` を `lto = "thin"` に維持

### 2026-04-23 更新内容
- 上流 `rust-v0.123.0` を取り込み、`codex-rs` を同期
- 再適用した差分:
  - `core/login/ollama` の `reqwest` に `native-tls-vendored`
  - `login` / `rmcp-client` のブラウザ起動を `open_url` ベースに統一
  - `rmcp-client` の Termux/Android 環境変数保持
  - Android では `code-mode` 依存と `exec` / `wait` 公開を無効化
  - `arg0` の Android `try_lock()` 回避
  - `core/src/installation_id.rs` の Android `file.lock()` 回避（新規: Rust 1.87+ の `File::lock` 対応）
  - `tui` の Android 向け警告抑止
  - `codex-rs/Cargo.toml` の `[profile.release]` を `lto = "thin"` に維持

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
- 対処: `rmcp-client/src/utils.rs`の`DEFAULT_ENV_VARS`を確認
  - 特に、Termux環境変数（`TERMUX_VERSION`, `PREFIX`, `LD_LIBRARY_PATH` など）が含まれているか確認

### 8.2 ブラウザが自動で開かない
- Termux: `termux-open-url`コマンドがインストールされているか確認（`pkg install termux-api`）
- WSL/SSH/Container: 手動でURLを開く必要がある（設計通りの動作）

### 8.3 TLSエラーが発生
- 症状: HTTPSリクエストでTLS/SSL関連のエラー、または Android クロスビルドで
  `openssl-sys` が対象環境の OpenSSL を検出できず失敗
- 対処:
  - `http-client/Cargo.toml` の `reqwest` で `native-tls-vendored` が有効か確認
  - `cargo tree --target aarch64-linux-android -i openssl-src -e features` で
    `openssl-sys` から vendored OpenSSL へ到達することを確認

### 8.4 Androidで "try_lock() not supported" 警告が出る
- 症状: `WARNING: proceeding, even though we could not update PATH: try_lock() not supported`
- 原因: Androidのファイルシステムが `flock()` をサポートしていない
- 対処: `arg0/src/lib.rs` の `try_lock()` 呼び出しを `#[cfg(not(target_os = "android"))]` で除外

### 8.7 Androidで "thread/start failed during TUI bootstrap: lock() not supported" が出る
- 症状: `codex` 起動時に `Error: thread/start failed during TUI bootstrap` で即終了
- 原因: `core/src/installation_id.rs` の `file.lock()` (Rust 1.87+ の `std::fs::File::lock`) がAndroidで非対応
- 対処: `file.lock()?;` を `#[cfg(not(target_os = "android"))]` で除外
- 補足: エラーメッセージに `thread-writer-locks/.coordination.lock` が含まれる場合は
  `thread-store/src/local/writer_lock.rs`（0.147.0 で新設）のロックが原因。§3.6 の対処を適用する

### 8.8 Android で gpt-5.6 系が「会話はできるがツールを一切使わない」
- 症状: ファイル操作・コマンド実行を伴う指示に応答しない / `Code Mode is unavailable ... Code mode will fail closed` の警告
- 原因: gpt-5.6 系はサーバー側メタデータで `tool_mode: code_mode_only` が指定され、
  `codex-code-mode-host` を提供できない Android では upstream 仕様上 fail-closed になる
- 対処: `core/src/tools/mod.rs` の `effective_tool_mode` に Android 強制 Direct ガードを適用（§3.9）
- 補足: `[features] code_mode = false` 等の設定では回避不可（モデルメタデータが feature より優先）
- Direct フォールバック後の
  `Code Mode is unavailable ... Falling back to direct tools` は機能障害ではないが、
  起動のたびに表示されるため、`core/src/tools/code_mode/mod.rs` で
  Android かつ実効モードが Direct の場合のみ抑制する（§3.9）

### 8.5 Androidビルドで unused/dead_code 警告が出る
- 症状: `tui/src/clipboard_paste.rs` で `unused import: tempfile::Builder` / `dead_code` 警告が出る
- 対処: Android向けの条件付きコンパイルを適用
  - `#[cfg(not(target_os = "android"))] use tempfile::Builder;`
  - `#[cfg_attr(target_os = "android", allow(dead_code))]` を `PasteImageError` と Android版 `paste_image_as_png` に付与

### 8.6 Androidビルドで exit code 143 が出る
- 症状: GitHub Actions の `Build Android` ジョブで `Build` ステップ終盤に `Process completed with exit code 143`
- 主因候補: release リンク工程の長時間化（特に `lto = "fat"`）
- 対処:
  - `codex-rs/Cargo.toml` の `[profile.release]` を `lto = "thin"` に設定
  - 修正後に再 push し、`Build Android` が成功することを確認

---

このガイドに沿って差分を適用すれば、上流更新のたびに同様のクロスプラットフォーム対応を素早く再現できます。追加で対応が必要になった環境が出てきた場合は、本ドキュメントに追記してください。

### 2025-09-18 更新内容
- `core/src/util.rs` の `open_url` が `OpenUrlStatus` / `OpenUrlError` を返すようになり、環境制約時は `Suppressed` で通知
- Linux系では `BROWSER`・`gio open`・`sensible-browser`・Firefox/Chrome/Chromium を順にフォールバック
- `login/src/server.rs` は `OpenUrlStatus::Suppressed` を受けて自動で案内メッセージと URL を表示
- `mcp-client/src/mcp_client.rs` の `DEFAULT_ENV_VARS` に Android / Termux 向けの詳細な環境変数を明示

### 2025-09-24 更新内容
- 実装の完了確認とドキュメント同期
- GitHub Actions での全プラットフォームビルド成功を確認（Linux/macOS/Windows/Android）
- `core/src/util.rs` の `open_url` 関数を公開APIとして完全実装
- `login/src/server.rs` の `webbrowser` 依存削除と `codex_core::util::open_url` への移行完了
- 実装したクロスプラットフォーム対応が正常に機能することを確認

### 2025-09-25 更新内容
- ドキュメントと実装の完全な同期を実現
- `reqwest` への `native-tls-vendored` 機能を確実に追加（`core`、`login`、`ollama` の各 `Cargo.toml`）
- `core/src/util.rs` の `open_url` 関数実装を完了：
  - `OpenUrlStatus` と `OpenUrlError` の型定義
  - 環境検知関数（`is_termux`、`is_wsl`、`is_ssh`、`is_container`）の実装
  - プラットフォーム別のブラウザ起動ロジック（Linux/macOS/Windows/Termux）
- `login/src/server.rs` で `webbrowser` 依存を完全に削除し、`open_url` を使用
- `mcp-client/src/mcp_client.rs` の `DEFAULT_ENV_VARS` に Termux/Android 環境変数を追加：
  - `TERMUX_VERSION`、`PREFIX`、`TERMUX_APK_RELEASE`、`TERMUX_APP_PID`
  - `ANDROID_ROOT`、`ANDROID_DATA`
  - `LD_LIBRARY_PATH`、`LD_PRELOAD`（動的リンクに必須）
- クロスプラットフォーム対応の実装が完全にドキュメント記載通りに実現

### 2025-10-10 実装完了
- ドキュメントに基づいて、すべてのクロスプラットフォーム対応を実装完了
- **TLS依存関係**: `core`、`ollama`、`login` の各 `Cargo.toml` に `native-tls-vendored` を追加完了
- **ブラウザ起動処理**:
  - `core/src/util.rs` に完全な `open_url` 関数を実装（環境検知、エラーハンドリング含む）
  - `login/src/server.rs` を `codex_core::util::open_url` を使用するように更新
  - `login/Cargo.toml` から `webbrowser` 依存を削除
  - `OpenUrlStatus::Suppressed` 時に適切なメッセージとURLを表示する処理を実装
- **MCP環境変数**: `mcp-client/src/mcp_client.rs` の `DEFAULT_ENV_VARS` に Termux/Android 環境変数を追加完了
- すべての実装がドキュメント記載の仕様に準拠していることを確認

### 2026-02-06 更新内容
- Androidビルドの警告抑止対応を追記
  - `tui/src/clipboard_paste.rs` の `tempfile::Builder` import を非Androidに限定
  - `PasteImageError` / Android版 `paste_image_as_png` に `allow(dead_code)` を付与

### 2026-02-19 更新内容
- 上流 `rust-v0.107.0` 同期時の再適用ポイントを反映
  - `core/login/ollama` の `reqwest` に `native-tls-vendored` を再適用
  - `login` / `rmcp-client` のブラウザ起動を `open_url` ベースに再適用
  - `rmcp-client` の Termux/Android 環境変数保持を再適用
  - `arg0` / `tui` の Android 向け条件分岐を再適用
- GitHub Actions `Build Android` での `exit code 143` 対策として
  `codex-rs/Cargo.toml` の `[profile.release]` を `lto = "thin"` に設定し、Androidビルド成功を確認

### 2026-03-06 更新内容
- 上流 `rust-v0.111.0` を取り込み、`codex-rs` を同期
- 再適用した差分:
  - `core/login/ollama` の `reqwest` に `native-tls-vendored`
  - `login` / `rmcp-client` のブラウザ起動を `open_url` ベースに統一
  - `rmcp-client` の Termux/Android 環境変数保持
  - `arg0` の Android `try_lock()` 回避
  - `tui` の Android 向け警告抑止
  - `codex-rs/Cargo.toml` の `[profile.release]` を `lto = "thin"` に維持

### 2026-04-09 更新内容
- 上流 `rust-v0.118.0` を取り込み、`codex-rs` を同期
- `login` / `rmcp-client` のブラウザ起動ロジックを Termux / WSL / SSH / Container 対応へ再適用
- `arg0` の Android `try_lock()` 回避、`tui` の Android 警告抑止、`profile.release.lto = "thin"` を再適用

### 2026-03-16 更新内容
- 上流 `rust-v0.114.0` を取り込み、`codex-rs` を同期
- 再適用した差分:
  - `core/login/ollama` の `reqwest` に `native-tls-vendored`
  - `login` / `rmcp-client` のブラウザ起動を `open_url` ベースに統一
  - `rmcp-client` の Termux/Android 環境変数保持
  - `arg0` の Android `try_lock()` 回避
  - `tui` の Android 向け警告抑止
  - `codex-rs/Cargo.toml` の `[profile.release]` を `lto = "thin"` に維持

### 2025-10-24 実装完了・ドキュメント同期
- ドキュメントと実装の完全な同期を確認・修正
- **TLS依存関係**:
  - `core/Cargo.toml` の `reqwest` に `"native-tls-vendored"` を追加
  - `login/Cargo.toml` の `reqwest` に `"native-tls-vendored"` を追加
  - `ollama/Cargo.toml` の `reqwest` に `"native-tls-vendored"` を追加
- **ブラウザ起動処理**:
  - `core/src/util.rs` に `open_url` 関数を完全実装
    - `OpenUrlStatus` と `OpenUrlError` 型を定義
    - 環境検知関数を実装（`is_termux`, `is_wsl`, `is_ssh`, `is_container`）
    - プラットフォーム別のブラウザ起動ロジック（Linux/macOS/Windows/Android/Termux）
  - `login/src/server.rs` を修正
    - `codex_core::util::{open_url, OpenUrlStatus}` をインポート
    - `webbrowser::open` を `open_url` に置き換え
    - `OpenUrlStatus::Suppressed` 時にメッセージを表示
  - `login/Cargo.toml` から `webbrowser` 依存を削除
- **MCP環境変数**:
  - `rmcp-client/src/utils.rs` の `DEFAULT_ENV_VARS` に Termux/Android 環境変数を追加
  - 追加した環境変数：`TERMUX_VERSION`, `PREFIX`, `TERMUX_APK_RELEASE`, `TERMUX_APP_PID`, `ANDROID_ROOT`, `ANDROID_DATA`, `LD_LIBRARY_PATH`, `LD_PRELOAD`
- すべての実装がドキュメント記載の仕様に完全に準拠
