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
- ディレクトリ読み込み (`read_directory`) はバックグラウンドスレッドで非同期実行し、読み込み中はスピナーを表示。generation カウンタで古い結果を破棄
- **UIスレッドで `path.exists()` や `fs::metadata()` など I/O ブロッキング呼び出しを行わないこと。** HDD/ネットワーク/WSL ドライブでは数秒かかる場合がある。`navigate_to_with_resolver` でパス解決も含めてバックグラウンドで実行する
- ディレクトリの自動リフレッシュは `fs::metadata().modified()` の mtime ポーリング（2秒間隔）で実現。カーソル位置・選択状態はファイル名で復元
- ディレクトリ毎のカーソル位置は `cursor_history` でファイル名ベースで保存し、再訪・再起動時に復元。`cursor_dirty` で変更追跡し、両パネル間の上書きを防止。`loading_old_name` は上方向移動時のみ設定

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

### 整合性の維持
- キーバインドを追加・変更したら `handle_misc_keys` 内のヘルプテキスト（`?` キー）も必ず更新する
- コメントにキー名を含む場合（例: `// Ctrl+.: toggle hidden`）、キー変更時にコメントも更新する

### Single Source of Truth（状態の一元管理）
- 同じデータを複数箇所に複製すると、一方の更新が他方の古い値で上書きされる。共有データは1箇所で管理するか、dirty tracking で変更箇所のみ永続化する
- 「この値は誰が所有し、誰が更新するか」を明確にしてから実装する

### 保存と復元のライフサイクル対称性
- 「いつ保存するか」と「いつ復元するか」は対で設計する。イベント駆動（ナビゲーション時）の保存だけでは、イベントなしで終了するケースを見落とす
- ライフサイクル境界（起動・終了）での保存・復元を必ず検討する

### 関数のスコープを設計意図に一致させる
- 「特定の条件でのみ有効な値」を常に設定すると、意図しないコンテキストで副作用が発生する。条件分岐で設定スコープを制約する
- センチネル値（`".."` など）を通常データと同じ経路で永続化しない。保存前にバリデーションを行う

### Security
- ユーザー入力のパスは必ず検証する
- ファイル削除はトラッシュ（ゴミ箱）経由で行う（`trash` crate）。UNC パスはゴミ箱非対応のため直接削除にフォールバックし、確認ダイアログで警告表示
- 破壊的操作（削除、上書き）は必ず確認ダイアログを表示する
- Windowsファイル属性の安全な読み取り
- パストラバーサル攻撃を防ぐ

## Debugging Methodology
- **実データから始める**: save/restore のバグでは、まず永続化されたデータ（config.json 等）を確認する。コードパスの理論的推論より、実際の状態の観察が速く正確
- **フルサイクルで検証する**: 個別の関数（save / restore）が正しくても、パイプライン全体（変更→保存→永続化→読込→復元）が壊れていれば無意味。端から端まで通して確認する
- **修正と検証を分離する**: バグを修正したら、ユーザーに確認させる前に自分で検証する。「もっともらしい修正」が実際の症状の原因とは限らない

## Continuous Improvement
- このCLAUDE.md自体を常に改善・更新していく（設計判断、環境の注意点、ワークフローの変更など）
- コミット前に作業を振り返り、得られた知見があれば CLAUDE.md / MEMORY.md / settings.local.json に記録する
- 既存の記録が古くなっていたら更新・削除する
