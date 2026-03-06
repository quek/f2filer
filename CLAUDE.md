# f2filer - Dual-Pane File Manager

## Project Overview
Rust + egui (eframe 0.31) で作成された2画面ファイラー。

## Tech Stack
- Language: Rust (edition 2021)
- GUI: eframe 0.31 / egui
- Font: HackGenConsoleNF-Regular.ttf（ユーザーローカルフォント）
- Dependencies: chrono, open, trash, serde, serde_json, image, hound, rodio

## Architecture
```
src/
├── main.rs          # エントリポイント (eframe起動、ウィンドウ位置/サイズ復元)
├── app.rs           # メインアプリ構造体、キーボードハンドリング、ダイアログ結果処理、フォント設定
├── panel.rs         # FilePanel: ファイル一覧表示、カーソル、選択、フィルター、中央省略表示
├── file_item.rs     # FileItem構造体、ディレクトリ読み込み、隠しファイル判定
├── file_ops.rs      # ファイル操作 (コピー/移動/削除/リネーム)、ドライブ列挙
├── dialog.rs        # ダイアログ (確認/入力/メッセージ/ドライブ選択)
├── sort.rs          # ソートロジック (名前/拡張子/サイズ/日付)
├── config.rs        # 設定の永続化 (APPDATA/f2filer/config.json)
├── image_viewer.rs  # 画像プレビュー (静止画+GIFアニメ、非同期読込、LRUキャッシュ)
├── audio_viewer.rs  # WAV波形表示+再生 (ストリーミング再生、無音スキップ、バックグラウンド波形読込)
└── viewer.rs        # テキストファイルビューア
```

## Keybindings
- `j`/`k`/`↑`/`↓`: カーソル移動
- `l` / `Enter`: ディレクトリを開く / ファイル実行
- `h`: 親ディレクトリ
- `i`: パネル切替
- `Space`: 選択トグル
- `Ctrl+A`: 全選択
- `f`: フィルターにフォーカス（Enter: 確定して最初のマッチに移動、Escape: キャンセル）
- `o`: 反対側パネルを同期
- `c`: コピー (選択ファイルのみ)
- `m`: 移動 (選択ファイルのみ)
- `d`: 削除 (選択ファイルのみ、トラッシュ)
- `r`: リネーム
- `n`: 新規ディレクトリ
- `p`: ドライブ選択（ドライブレターキーで直接選択）
- `v`: プレビュー切替（画像/WAV波形+再生、反対パネルに表示、カーソル追従）
- `g`: 登録ディレクトリ一覧
- `Shift+G`: 現在のディレクトリを登録
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

# プロセスが残っている場合（PowerShell推奨）
powershell -Command "Stop-Process -Name f2filer -Force -ErrorAction SilentlyContinue"
cargo build
```

Note: bash上で `taskkill /F /IM f2filer.exe` はワーキングディレクトリが `F:/` の場合パース失敗するため、PowerShellの `Stop-Process` を使用する。

## Config (APPDATA/f2filer/config.json)
- `show_hidden`: 隠しファイル表示
- `last_left_dir` / `last_right_dir`: 左右パネルの最後のディレクトリ
- `drive_dirs`: ドライブごとの最後にいたディレクトリ (HashMap)
- `registered_dirs`: 登録ディレクトリ (Vec<RegisteredDir>: key, name, path)
- `window_x` / `window_y` / `window_width` / `window_height`: ウィンドウ位置・サイズ

設定は毎回のディレクトリナビゲーション時に保存される（`taskkill /F` は `on_exit` を呼ばないため）。

## Design Decisions
- コピー/移動/削除はSpaceで選択したファイルのみ対象（カーソル位置のファイルは対象外）
- コピー/移動先に同名ファイルがある場合は上書き確認ダイアログを表示
- レイアウトは `ui.columns(2, ...)` を使用（`ui.horizontal` + `ui.vertical` は高さが正しく配分されない）
- ファイルリストのカラムは `allocate_ui_with_layout` で配置制御（`add_sized` は中央揃えになるため使わない）
- 長いファイル名は中央省略で表示（`truncate_middle` 関数）。文字幅は `ui.fonts()` でモノスペースフォントのグリフ幅を測定して動的に計算
- フィルターにフォーカス中はキーボードショートカットを無効化（`filter_has_focus` フラグ）
- フィルター入力中はマッチするファイルにカーソル自動移動（`..` はスキップ）
- フィルターのEnter検出は `response.lost_focus()` を使用（egui の singleline TextEdit は Enter で自動的にフォーカスを手放すため `has_focus()` は使えない）
- `?`キーの検出はテキストイベント (`egui::Event::Text`) を使用（Shift+Slashはキーボード配列依存）
- ドライブ選択はドライブレターキーで直接選択
- ドライブ切替時は前回そのドライブで最後にいたディレクトリを復元
- 画像プレビューは反対パネルに表示し、カーソル移動に追従
- 画像の読み込みはバックグラウンドスレッドで非同期実行（`Arc<Mutex<Option<DecodedImage>>>`）
- 画像キャッシュはLRU方式（最大20エントリ）、`wanted_path` で古い読み込み結果の表示を防止
- GIFアニメーションは全フレームをデコードし、`Instant::now()` ベースのタイマーでループ再生
- フォントは HackGenConsoleNF を `setup_fonts()` で Proportional/Monospace 両方に設定
- ウィンドウ位置・サイズは毎フレーム `viewport().outer_rect` / `inner_rect` で追跡し、config保存時に永続化
- 登録ディレクトリはカスタムショートカットキー付き（デフォルト: ディレクトリ名の先頭文字）
- WAVプレビューは再生（rodio ストリーミング）と波形読み込み（hound バックグラウンドスレッド）を分離して即時再生
- WAV再生時は先頭の無音部分を自動スキップ（閾値 0.01）
- ファイルリストのカラムは `ui.painter().text()` で直接ピクセル位置に描画（レイアウトシステムをバイパス）

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
