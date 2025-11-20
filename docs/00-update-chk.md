#以下を各ステップごとに実施して

- 現状実装バージョンは0.60.1

- https://github.com/openai/codex.gitのリリースを確認

- 現状の実装バージョンと比較して、新しければソースコードをダウンロード

- 現状実装の　codex-rs　を　ソースコード内の　codex-rs　に置き換え

- ./docs/01-cross-platform-update-guide.mdに従って、現状実装を修正

- 修正後はビルドテストのためgit status commit push

- ghでactions log確認し、エラーがあれば修正

- アンドロイド版のビルドが完了したら、ダウンロード

- 現状実装バージョンを更新
