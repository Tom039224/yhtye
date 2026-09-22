# アーキテクチャの記録

`docs/design/` が Claude Design から起こした**画面**の仕様であるのに対し、
ここは Yhtye の**機能**の設計を記録する。進め方は [`PLAN.md`](../../PLAN.md)。

| ファイル | 内容 |
|---|---|
| [`orchestration-model.md`](orchestration-model.md) | Group / Task / Step のモデル、状態機械、git、権限、永続化 (確定事項) |
| [`mcp-tools.md`](mcp-tools.md) | Yhtye がホストする MCP サーバーのツール仕様 (引数・戻り値・エラー・遷移) |
| [`core-design.md`](core-design.md) | crate / モジュール構成、公開 API、transport、WS ブリッジ、偽エージェント、テスト方針 |
| [`acp-harnesses.md`](acp-harnesses.md) | ACP、Rust クライアント crate の使い方、Claude Code アダプタの調査結果、フォールバック |

## 前提

- 画面は Tauri 2 + React。ドメインと I/O は Tauri 非依存の Rust crate `yhtye-core` に置く。
- ハーネスとは [ACP (Agent Client Protocol)](https://agentclientprotocol.com) でつなぐ。
  Yhtye は ACP **クライアント**として、オーケストレータとサブエージェントの
  セッションを並べて管理する。
- タスク分割・順序・方針の判断はオーケストレータ役のハーネス (LLM) が行う。
  ただし判断の**表明**は Yhtye がホストする MCP サーバーのツール呼び出しに限り、
  工程間の機械的な遷移・git 操作・権限応答は Yhtye が行う。
