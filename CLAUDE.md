# f2filer - Dual-Pane File Manager

## Project Overview
Rust + egui (eframe 0.31) で作成された2画面ファイラー。NyanFi風のキーバインドを持つ。

## Tech Stack
- Language: Rust (edition 2021)
- GUI: eframe 0.31 / egui
- Dependencies: chrono, open, trash, serde, serde_json

## Architecture
```
src/
├── main.rs       # エントリポイント (eframe起動)
├── app.rs        # メインアプリ構造体、キーボードハンドリング、ダイアログ結果処理
├── panel.rs      # FilePanel: ファイル一覧表示、カーソル、選択、フィルター
├── file_item.rs  # FileItem構造体、ディレクトリ読み込み、隠しファイル判定
├── file_ops.rs   # ファイル操作 (コピー/移動/削除/リネーム)、ドライブ列挙
├── dialog.rs     # ダイアログ (確認/入力/メッセージ/ドライブ選択)
├── sort.rs       # ソートロジック (名前/拡張子/サイズ/日付)
├── config.rs     # 設定の永続化 (APPDATA/f2filer/config.json)
└── viewer.rs     # テキストファイルビューア
```

## Keybindings
- `j`/`k`/`↑`/`↓`: カーソル移動
- `l`: ディレクトリを開く / ファイル実行
- `h`: 親ディレクトリ
- `i`: パネル切替
- `Space`: 選択トグル
- `Ctrl+A`: 全選択
- `f`: フィルターにフォーカス
- `o`: 反対側パネルを同期
- `c`: コピー (選択ファイルのみ)
- `m`: 移動 (選択ファイルのみ)
- `d`: 削除 (選択ファイルのみ、トラッシュ)
- `r`: リネーム
- `n`: 新規ディレクトリ
- `p`: ドライブ選択
- `F3`: テキストビューア
- `Ctrl+R`: リフレッシュ
- `Ctrl+.`: 隠しファイル表示切替
- `Ctrl+Q`: 終了
- `?`: ショートカット一覧

## Development Workflow
```bash
# ビルド
cargo build

# 実行
cargo run

# プロセスが残っている場合
taskkill /F /IM f2filer.exe
cargo build
```

## Design Decisions
- コピー/移動/削除はSpaceで選択したファイルのみ対象（カーソル位置のファイルは対象外）
- コピー/移動先に同名ファイルがある場合は上書き確認ダイアログを表示
- レイアウトは `ui.columns(2, ...)` を使用（`ui.horizontal` + `ui.vertical` は高さが正しく配分されない）
- フィルターにフォーカス中はキーボードショートカットを無効化
- `?`キーの検出はテキストイベント (`egui::Event::Text`) を使用（Shift+Slashはキーボード配列依存）
- ドライブ選択はドライブレターキーで直接選択

## Coding Principles

### KISS (Keep It Simple, Stupid)
- 最小限の実装で目的を達成する
- 不要な抽象化やラッパーを作らない
- 1つの関数は1つの責務に集中する
- 過剰な設計より動くコードを優先する

### DRY (Don't Repeat Yourself)
- 共通処理は関数に抽出する（例: `copy_file_or_dir_inner` で通常コピーと上書きコピーを共通化）
- 定数やマジックナンバーは変数として定義する
- パターンが3回繰り返されたら抽象化を検討する

### Security
- ユーザー入力のパスは必ず検証する
- ファイル削除はトラッシュ（ゴミ箱）経由で行う（`trash` crate）
- 破壊的操作（削除、上書き）は必ず確認ダイアログを表示する
- Windowsファイル属性の安全な読み取り
- パストラバーサル攻撃を防ぐ
