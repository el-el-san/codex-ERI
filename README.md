<h1 align="center">Codex CLI -Extended Resource Integration</h1>

## このプロジェクトについて

このプロジェクトは [OpenAI Codex](https://github.com/openai/codex) のフォークであり、動作改善と機能拡張を行ったカスタム版です。

### 法的通知
- オリジナル著作権: 2025 OpenAI
- 改変: 2025 ERU  
- ライセンス: Apache License 2.0 (LICENSE ファイル参照)

元のバージョンを正確に追跡できないため、すべてのファイルは元の状態から変更されているものとみなしてください。

## 主な追加機能

### 🌐 HTTP MCP統合
- **セッションID対応**: HTTP MCPのセッション管理機能を実装
- **動的サービス追加**: `config.toml`での簡単なMCPサーバー設定
- **バッチ処理接続**: 複数のMCPサーバーを段階的に接続（最大5並列）
- **接続タイムアウト拡張**: 60秒のタイムアウトで安定した接続

### 💬 会話機能の拡張
- **非インタラクティブ会話継続 (-r モード)**: 
  - `codex exec -r "続きの指示"` で前回の会話を継続
  - rollout機能を使用した完全な会話履歴の保持
  - GPT応答も含めた文脈の維持

### ⌨️ 対話モードの改善
- **Working中の入力キューイング**: 
  - AIが処理中でも追加の入力が可能
  - 入力はキューに保存され、タスク完了後に自動処理
  - 画面にキュー数と現在の入力状態を表示
  - 効率的な連続タスク処理を実現

- **カスタムスラッシュコマンド**: 
  - ユーザー定義のスラッシュコマンドを追加可能
  - `config.toml`で簡単に設定
  - シェルコマンド実行とプロンプト送信の2種類のタイプ
  - ビルトインコマンドと同様にポップアップ表示とタブ補完に対応

### 🚀 モデル仕様の最適化
- **GPT-5コンテキスト長の正確な設定**:
  - コンテキストウィンドウ: 400,000トークン（正式仕様に準拠）
  - 最大出力トークン: 128,000トークン（正式仕様に準拠）
  - 大規模なコードベース解析や長文処理の性能向上

### ⚡ ファイル読み取りの並列実行
- **複数ファイルの同時読み取り**:
  - GPT-5/GPT-4oモデルで並列実行が有効
  - 2ファイル読み取りで約40-57%高速化
  - config.tomlで並列実行数を調整可能（デフォルト: 5）
  - 読み取り専用操作の自動並列化

## インストールとビルド

### 必要な環境
- Rust 1.70以降
- Git

### ビルド手順

```bash
# リポジトリのクローン
git clone https://github.com/el-el-san/codex
cd codex/codex-rs

# 標準的なビルド
cargo build --release --workspace

```

### バイナリの所在

ビルド完了後、実行可能バイナリは以下の場所に生成されます：

```
codex-rs/target/release/
├── codex      # メインCLIバイナリ
├── codex-exec # 実行エンジンバイナリ
└── codex-tui  # TUI版バイナリ
```

### インストール（オプション）

```bash
# システムパスにインストール
cargo install --path cli/
cargo install --path exec/
cargo install --path tui/

# または直接実行
./target/release/codex 
```

## 基本的な使い方

```bash
#対話モードで実行
codex

# 非インタラクティブでシンプルなタスク実行
codex exec "ファイルをリストして"

# 会話の継続 (-r オプション)
codex exec "プロジェクトの構造を分析して"
codex exec -r "src/ディレクトリの詳細を見せて"

```


## 設定

### MCP サーバー設定 (`config.toml`)

```toml
# MCP設定例
[mcp_servers.name]
url = "https://xxx"

# タイムアウト設定
mcp_tool_timeout_ms = 30000
```

### 並列実行設定 (`config.toml`)

```toml
# 並列実行の設定
[parallel_execution]
enabled = true                # 並列実行の有効/無効化
max_concurrent_calls = 5      # 同時実行可能なツール呼び出し数（デフォルト: 5）
min_delay_ms = 100            # API呼び出し間の最小遅延（ミリ秒）
max_retries = 5               # 最大リトライ回数
```

### カスタムスラッシュコマンド設定 (`config.toml`)

```toml
# カスタムコマンドの例
[[custom_commands]]
name = "build"
description = "プロジェクトをビルド"
type = "shell"
content = "./resume-build.sh"
parallel = false              # 並列実行の可否（オプション）
depends_on = []              # 依存関係（オプション）

[[custom_commands]]
name = "test"
description = "テストを実行"
type = "shell"
content = "cargo test"
parallel = false
depends_on = ["build"]       # buildコマンド完了後に実行

[[custom_commands]]
name = "docs"
description = "ドキュメントを生成"
type = "prompt"
content = "このプロジェクトのREADMEを更新してください"

[[custom_commands]]
name = "analyze"
description = "コード分析を実行"
type = "prompt"
content = "現在のコードベースを分析して問題点を指摘してください"
```

**カスタムコマンドのタイプ:**
- `shell`: シェルコマンドとして実行（結果は画面に表示）
- `prompt`: LLMへのプロンプトとして送信

**並列実行オプション:**
- `parallel`: trueの場合、他のコマンドと並列実行可能
- `depends_on`: 指定したコマンドの完了を待ってから実行


