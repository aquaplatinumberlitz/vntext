# Phát triển vntext và profile game

Source build: [INSTALL_VI](INSTALL_VI.md). Cách dùng và game đã hỗ trợ:
[USER_GUIDE_VI](USER_GUIDE_VI.md). Ranh giới backend/client: [ARCHITECTURE_VI](ARCHITECTURE_VI.md).
Quy tắc làm việc ở [PLAN mục 1–3](PLAN.md); đọc trước khi sửa engine hoặc chạy test live.

## Cấu trúc dự án

```
src/
  main.rs            Dòng lệnh, vòng đọc theo profile, luồng dịch
  launcher.rs        Cửa sổ chọn game (GTK4); chạy phần đọc như tiến trình con
  tray.rs            Icon khay hệ thống, và việc chỉ cho một cửa sổ chọn game chạy
  install.rs         `vntext install`: mục trong menu ứng dụng, icon, liên kết ở ~/.local/bin
  doctor.rs          `vntext doctor` và khung kiểm tra máy; nút "Cho phép" đọc bộ nhớ
  explain.rs         Đổi chuỗi lỗi gốc thành câu tiếng Việt và việc nên làm
  vntext.svg         Icon (kèm hai bản điểm ảnh cho khay)

  mem.rs             Đọc bộ nhớ process khác, tìm thư mục game
  text.rs            Tìm chuỗi tiếng Nhật trong dữ liệu thô (cho strings, find, watch)
  engines/
    mod.rs           Giao diện `Engine` chung và danh sách `DETECTORS`
    <tên>/mod.rs     Một profile: tìm script, theo dõi vị trí đang đọc
    <tên>/text.rs    Định dạng gói, script và text của engine đó
                     (age, ark, bishop, catsystem, escude, liar, med, ninetail, shin, spt, yuris)

  ocr.rs             Đọc khung thoại từ ảnh chụp qua Gemini
  screen.rs          Hỏi desktop về cửa sổ game: bản X11, bản KWin, bản ghép; nén ảnh
  kwin.rs            Backend KWin: script báo vị trí và focus, chụp qua ScreenShot2

  overlay.rs         Dải nổi, cụm nút, popup tra từ và hộp thiết lập (GTK4, layer-shell)
  translate.rs       Các nguồn dịch, cache bản dịch
  yomitan.rs         Tra qua API của Yomitan; đổi nội dung từ điển thành markup cho popup
  dict/
    mod.rs           `candidates`: mọi chuỗi mà từ điển có thể liệt kê cho một đoạn chữ
    deinflect.rs     Máy chia ngược, bản Rust của LanguageTransformer của Yomitan
    variants.rs      Các cách viết khác của một đoạn chữ (nửa rộng, kana, romaji…)
    japanese.json    Luật và bảng ký tự xuất từ Yomitan
    vectors.json     Kết quả của chính Yomitan trên khoảng 19.000 mẫu, để test đối chiếu
    store.rs         Kho từ điển SQLite
    import.rs        Nhập file zip từ điển của Yomitan
    lookup.rs        Tra đoạn chữ trong kho hoặc qua Yomitan
  web.rs             `Hub`: kênh sự kiện chung của tool, và máy chủ trang đọc
  tts.rs             Client A.I.VOICE2, câu chờ mới nhất, phát WAV trong RAM bằng pw-play
  tts_audio.rs       Luồng PCM/PipeWire giữ sẵn, bộ đệm giới hạn, dừng đúng player
  page.html          Trang đọc cho trình duyệt
tools/
  yomitan-dump.mjs   Xuất luật của Yomitan ra japanese.json và vectors.json
docs/
  README.md          Chỉ mục tài liệu hiện hành và hồ sơ lịch sử
  ARCHITECTURE_VI.md  Ranh giới ba repo và source của vntext
  TTS_CLIENT_VI.md    Kết nối TTS, phát/cache/lookahead và test client
  engines.md         Ghi chú kỹ thuật từng engine
  PLAN.md            Kế hoạch đã dùng để làm phần giao diện cho người mới
```

Tool gồm ba tầng, chỉ tầng giữa phụ thuộc engine:

1. **Đọc bộ nhớ** (`mem.rs`). Liệt kê vùng nhớ từ `/proc/<pid>/maps`, đọc qua `/proc/<pid>/mem`, chỉ đọc trang đang có trong RAM và khử các vùng trùng nhau. Tốc độ quét khoảng 1 GB/giây.
2. **Tìm ra câu đang hiển thị** (`engines/`, hoặc `ocr.rs` cho game chưa có profile). Một profile đọc toàn bộ script của game thành danh sách đoạn thoại, rồi tìm trong bộ nhớ ô mà engine dùng để giữ vị trí đang đọc. Vòng đọc trong `main.rs` đọc ô đó mỗi 50 ms; ô trỏ sang đoạn khác thì đoạn mới được phát đi.
3. **Đưa text ra.** Mỗi đoạn được đẩy vào `Hub` (`web.rs`) dưới dạng sự kiện (`message`, `translation`, `notice`). Dải nổi, trang web và luồng dịch đều nghe từ đó, nên thêm một engine không phải sửa phần hiển thị.

Cách từng engine cất script và giữ vị trí đọc được ghi ở [docs/engines.md](engines.md).

Chạy test: `cargo test --release`. Cập nhật luật chia từ theo bản Yomitan mới: xem đầu file `tools/yomitan-dump.mjs`.

## Mở rộng engine khi gặp game mới

Profile engine là đường chính của tool: chỉ nó cho text đúng từng chữ, có tên người nói và dịch trước được. OCR chỉ là cách chơi tạm trong lúc chưa có profile.

### Bước 0: game có thật là engine mới không

1. Mở game rồi mở vntext. Nếu dòng của game ghi tên engine thì đã xong: nhiều hãng dùng chung engine (YU-RIS chẳng hạn có rất nhiều hãng dùng).
2. Nếu vẫn ghi "Chưa nhận diện" sau khi vào đoạn thoại và nhận diện lại, nhìn file trong thư mục game để biết nó thuộc họ nào. Bảng dưới là các engine phổ biến chưa có profile, vì tác giả chưa có game nào thuộc chúng để đo:

| Engine | File đặc trưng | Game ví dụ | Bắt đầu từ |
|---|---|---|---|
| KiriKiri 2 / KiriKiri Z | `*.xp3`, `data.xp3` | Fate/stay night, G-senjou no Maou, 11eyes, Amairo＊Islenauts, Hello Lady!, Clover Day's. Nhớ là: các game Yuzusoft sau này, nhiều game Lilith | [krkrz/krkrz](https://github.com/krkrz/krkrz) (mã nguồn engine), [xmoezzz/KrkrExtract](https://github.com/xmoezzz/KrkrExtract), `ArcFormats/KiriKiri` của GARbro. Gói thường mã hoá riêng theo từng game; script là `.ks` (text) hoặc `.scn` (PSB nhị phân) |
| SiglusEngine (VisualArt's) | `Scene.pck`, `Gameexe.dat`, `SiglusEngine.exe` | Nhớ là: Rewrite, Angel Beats!, game của SAGA PLANETS và Tone Work's | [xmoezzz/SiglusExtract](https://github.com/xmoezzz/SiglusExtract). `Scene.pck` nén và XOR bằng khoá riêng của game |
| Artemis | `*.pfs`, `root.pfs` | Boku no Elf Onee-san, Tsugou no Ii Kanojo, Memory's Dogma. Nhớ là: Hamidashi Creative, Aonatsu Line, 9-nine- | `ArcFormats/Artemis` của GARbro. Script là text (`.ast`, Lua), nên phần giải mã nhẹ |

   Game ví dụ lấy từ [bảng định dạng của GARbro](https://morkt.github.io/GARbro/supported.html); những tên sau chữ "nhớ là" chưa được kiểm chứng. Xếp theo số game thì KiriKiri đứng đầu, rồi tới Siglus, Artemis. CatSystem2 đã có profile (`catsystem`).

### Bước 1: tìm nghiên cứu có sẵn

Làm việc này trước khi tự mò; gần như engine nào cũng đã có người mổ xẻ.

- **GARbro** (`ArcFormats/`): định dạng gói và cách giải mã.
- **Công cụ dịch của cộng đồng** cho engine đó: thường có luôn định dạng script.
- **Textractor, LunaTranslator**: text nằm ở đâu, bảng mã gì.
- **Bản viết lại engine** nếu có (như `DCNick3/shin`, `krkrz`).
- **Game Unity kiểu Mono:** mở `Assembly-CSharp.dll` bằng ILSpy là đọc được mã của chính game.

### Bước 2: khảo sát game đang chạy

Cần trả lời ba câu hỏi:

1. **Script nằm ở đâu?** Trong RAM nguyên vẹn (shin, bishop), hay phải đọc từ gói trong thư mục game (đa số)?
2. **Text được mã hoá thế nào?** Bảng mã, mã điều khiển, furigana, tên người nói nằm ở đâu.
3. **Engine giữ vị trí đang đọc ở đâu?** Thường là một bản ghi của trình thông dịch: con trỏ hoặc số thứ tự lệnh, đứng cạnh vài giá trị nhận ra được (độ dài script, tên script, con trỏ tới đầu script).

Lệnh giúp khảo sát:

```
vntext strings <game>                    # mọi chuỗi tiếng Nhật trong bộ nhớ: cho biết bảng mã và mã điều khiển
vntext find <game> "một cụm đang hiện"   # địa chỉ của chuỗi đó, ở cả ba bảng mã
vntext watch <game> <địa chỉ> sjis       # xem một địa chỉ đổi thế nào khi click qua câu
```

Mẹo tìm ô vị trí: ghi lại giá trị nghi ngờ ở câu thứ nhất, click sang câu sau rồi so. Ô thật tiến đúng một bước mỗi lần click và đứng yên trong lúc câu đang hiện. Game thường giữ bản sao của trạng thái (dùng cho save); bản sao trỏ tới câu khác hoặc tụt lại phía sau.

### Bước 3: viết profile

Tạo thư mục `src/engines/<tên>/` với hai file. `text.rs` lo định dạng (mở gói, giải mã, tách script thành đoạn thoại); `mod.rs` lo phần nhìn vào bộ nhớ. Bộ khung của `mod.rs`:

```rust
//! One paragraph: which games, where the script is, how the position is found.

mod text;

use crate::engines::{Cell, Engine, Passage, Position};
use crate::mem::Proc;

pub struct Game {
    /// every passage of the script, in script order
    messages: Vec<String>,
}

/// Returns the profile if the process is a game of this engine.
pub fn detect(p: &Proc) -> Option<Box<dyn Engine>> {
    // cheapest test first: a file that only this engine has
    let pack = p.game_file(|name| name.eq_ignore_ascii_case("scene.pck"))?;
    let messages = text::script(&std::fs::read(pack).ok()?)?;
    Some(Box::new(Game { messages }))
}

impl Engine for Game {
    fn name(&self) -> String {
        "Tên engine".to_string()
    }

    fn len(&self) -> usize {
        self.messages.len()
    }

    fn passage(&self, index: usize) -> Passage {
        let raw = self.messages[index].clone();
        let (speaker, lines) = text::plain(&raw);
        Passage { speaker, lines, raw }
    }

    /// Scans the game's memory for the interpreter's record.
    fn locate(&self, p: &Proc, _thorough: bool) -> Position {
        let mut cells = Vec::new();
        let _ = p.scan(false, |base, data, limit, _| {
            for at in (0..limit.min(data.len().saturating_sub(16))).step_by(4) {
                if self.standing(&data[at..at + 16]).is_some() {
                    cells.push(Cell { addr: base + at as u64, trusted: true });
                }
            }
        });
        Position { cells }
    }

    /// Which passage the record at `addr` stands at now. Called 20 times a second.
    fn read(&self, p: &Proc, addr: u64) -> Option<usize> {
        let mut record = [0u8; 16];
        if p.read(addr, &mut record) != record.len() {
            return None;
        }
        self.standing(&record)
    }
}
```

`standing` là hàm của riêng bạn: nhận các byte của bản ghi, trả về số thứ tự đoạn thoại nếu chúng đúng là một bản ghi hợp lệ. Dùng chung nó cho cả `locate` và `read` thì hai chỗ không lệch nhau được.

Bốn hàm của `Engine`, và quy ước:

| Hàm | Việc | Lưu ý |
|---|---|---|
| `detect` | Nhận ra engine, nạp toàn bộ script | Trả `None` thật nhanh khi không phải engine này: mọi profile đều được thử với mọi game. |
| `len`, `passage(i)` | Số đoạn thoại và đoạn thứ `i` | Đoạn xếp theo thứ tự script, để `i + 1` là đoạn người đọc sắp thấy; dịch trước dựa vào điều này. |
| `locate` | Tìm các ô giữ vị trí đọc | Chạy ở luồng nền, được phép chậm. `trusted: false` cho ô chỉ "trông giống": vòng đọc sẽ chờ nó tiến như người đang đọc rồi mới tin. |
| `read` | Đoạn mà một ô đang trỏ tới | Phải nhanh. Trả `None` khi ô không còn hợp lệ; vòng đọc sẽ gọi lại `locate`. |

- **Tên người nói** để ở `Passage::speaker`, không lẫn vào `lines`. Lời dẫn thì `None`.
- **Khung lựa chọn** là một đoạn riêng, mỗi lựa chọn một dòng bắt đầu bằng `▶`.
- **`raw`** giữ text nguyên dạng engine lưu, cho cờ `--raw`.
- **Khác biệt giữa các game cùng họ** để thành bảng dữ liệu trong module (như `LAYOUTS` của shin), không thành nhánh code.
- **Thư mục game** lấy bằng `p.game_file(...)` / `p.game_files(...)`: nó tìm theo nơi chứa file exe đang nạp trong process, nên chạy đúng dưới mọi launcher.

### Bước 4: đăng ký

Trong `src/engines/mod.rs`, thêm `pub mod <tên>;` và thêm `<tên>::detect` vào `DETECTORS`. Profile chỉ nhìn file của game nên đứng trước profile phải quét bộ nhớ. Không phải sửa gì ngoài thư mục `engines/`.

### Bước 5: thử và ghi lại

1. `cargo build --release`, rồi `vntext auto <game> --debug --raw`. `--debug` in mỗi lần một ô vị trí đổi giá trị: click qua vài câu và so với màn hình.
2. Thử các tình huống dễ hỏng: load save, về màn hình chính rồi vào lại, tua nhanh, khung lựa chọn, xem lại cảnh.
3. Viết test cho phần giải mã trong `text.rs`, với dữ liệu tự dựng (đừng chép text của game vào repo).
4. Thêm game vào bảng [Game đã hỗ trợ](USER_GUIDE_VI.md#game-đã-hỗ-trợ), và ghi cách engine cất script và giữ vị trí vào [docs/engines.md](engines.md): đó là thứ người sau cần nhất khi một bản game mới làm profile hỏng.

Trong lúc chưa có profile, game vẫn chơi được bằng OCR, hoặc bằng Textractor chạy trong cùng prefix Wine.
