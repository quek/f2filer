# f2filer - Dual-Pane File Manager

プロジェクト概要、技術スタック、アーキテクチャ、キーバインド、設定については [README.md](README.md) を参照。

## Development Workflow

**重要: Bashコマンドに `cd` を付けないこと。** ワーキングディレクトリは常に `F:\dev\f2filer` に設定済み。

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
Note: MSYS2 bash環境から `make` を実行すると `link.exe` が `C:\WINDOWS` にtmpファイルを書けず失敗する。リリースビルドは `cargo build --release` を直接実行すること。

## Design Decisions
- コピー/移動/削除はSpaceで選択したファイルのみ対象（カーソル位置のファイルは対象外）
- コピー/移動先に同名ファイルがある場合は上書き確認ダイアログを表示
- レイアウトは `ui.columns(2, ...)` を使用（`ui.horizontal` + `ui.vertical` は高さが正しく配分されない）
- ファイルリストのカラムは `allocate_ui_with_layout` で配置制御（`add_sized` は中央揃えになるため使わない）
- 長いファイル名は中央省略で表示（`truncate_middle` 関数）。文字幅は `ui.fonts()` でモノスペースフォントのグリフ幅を測定して動的に計算
- フィルターにフォーカス中はキーボードショートカットを無効化（`filter_has_focus` フラグ）
- フィルター入力中はマッチするファイルにカーソル自動移動（`..` はスキップ）
- フィルターのEnter検出は `response.lost_focus()` を使用（egui の singleline TextEdit は Enter で自動的にフォーカスを手放すため `has_focus()` は使えない）
- `?`キー・`:`キーの検出はテキストイベント (`egui::Event::Text`) を使用（`key_pressed` は Shift 組み合わせで不安定）
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
- WSL ディストリビューションは `wsl.exe --list --quiet`（UTF-16LE）で検出しドライブ一覧に `WSL:distro` として統合（`read_dir` は UNC サーバー名に非対応）
- UNC パスの識別は `std::path::Prefix::UNC` を使用し、WSL 固有ではなく汎用的に処理
- UNC パス上のファイル削除はゴミ箱が使えないため `fs::remove_file` / `fs::remove_dir_all` にフォールバック
- UNC share root（`\\server\share`）からの上方ナビゲーションは Rust の `Path::parent()` が `None` を返すことで自然に防止される

## Coding Principles

### ベストプラクティスを追求する
- 最新のベストプラクティスでの実装を行なう

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
- ファイル削除はトラッシュ（ゴミ箱）経由で行う（`trash` crate）。UNC パスはゴミ箱非対応のため直接削除にフォールバックし、確認ダイアログで警告表示
- 破壊的操作（削除、上書き）は必ず確認ダイアログを表示する
- Windowsファイル属性の安全な読み取り
- パストラバーサル攻撃を防ぐ

## Continuous Improvement
- このCLAUDE.md自体を常に改善・更新していく（設計判断、環境の注意点、ワークフローの変更など）
- コミット前に作業を振り返り、得られた知見があれば CLAUDE.md / MEMORY.md / settings.local.json に記録する
- 既存の記録が古くなっていたら更新・削除する
