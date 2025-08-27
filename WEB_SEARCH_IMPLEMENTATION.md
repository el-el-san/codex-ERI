# WebSearch機能実装

## 概要
codex-v0.25を参考に、現在のcodex-ERIバージョンにWebSearch機能を追加しました。

## 実装内容

### WebSearchはOpenAIのビルトインツール
調査の結果、WebSearchはOpenAI APIが提供するビルトインツールであることが判明しました。これは：
- クライアント側で特別な実装は不要
- OpenAIがサーバー側でWeb検索を実行
- 検索結果は通常のメッセージとして返される

### 実装した変更

#### 1. ツール定義の追加
- `openai_tools.rs`: 
  - `OpenAiTool::WebSearch {}`列挙型を追加
  - `ToolsConfig`構造体に`web_search_request`フィールドを追加
  - `get_openai_tools`関数でWebSearchツールを条件付きで追加

#### 2. 設定項目
- `config.rs`: `include_web_search`フィールドを追加（デフォルト: false）

## 使用方法

設定ファイルで`include_web_search`をtrueに設定することで、WebSearchツールが有効になります：

```toml
# ~/.codex/config.toml
include_web_search = true
```

または、コード内で：

```rust
let tools_config = ToolsConfig::new(
    &model_family,
    approval_policy,
    sandbox_policy,
    include_plan_tool,
    true,  // include_web_search
);
```

## 動作原理

1. WebSearchツールがOpenAI APIに送信される
2. AIモデルがWeb検索を必要と判断した場合、OpenAIサーバーが自動的に検索を実行
3. 検索結果はAIの応答メッセージに含まれて返される
4. クライアント側では特別な処理は不要

## テスト

`openai_tools.rs`のテスト: ツール名リストに"web_search"が含まれることを確認

## 注意事項

- WebSearchはOpenAI APIのビルトイン機能のため、OpenAI以外のプロバイダーでは動作しない可能性があります
- 検索の実行と結果の取得はOpenAI側で完全に制御されます
- APIの利用料金に検索コストが含まれる場合があります