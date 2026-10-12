# vntext

Lấy text tiếng Nhật ra khỏi visual novel đang chạy để đọc cùng Yomitan và bản dịch máy.

Tool đọc bộ nhớ của game từ bên ngoài (`/proc/<pid>/mem`), không inject và không hook code, nên không phụ thuộc phiên bản emulator. Dự án bắt đầu vì hook kiểu Frida (agent của 0xDC00) gãy trên Ryujinx 1.3.3.

Chỉ chạy trên Linux. Dùng cá nhân, chưa công bố.

## Tình trạng

| Game | Engine | Nền tảng | Kết quả |
|---|---|---|---|
| Umineko no Naku Koro ni Saku | Entergram shin | Switch, Ryujinx 1.3.3 | Chính xác từng đoạn, đã kiểm tra |
| Higurashi no Naku Koro ni Hou | Entergram shin | Switch, Ryujinx 1.3.3 | Chính xác từng đoạn, đã kiểm tra |
| Game engine khác | chưa có profile | bất kỳ | Chỉ có chế độ quét chung, chưa dùng được (xem [Giới hạn](#giới-hạn)) |
| Game Windows qua Wine/Proton | — | — | Chưa thử lần nào |

## Dùng

```
cargo build --release
./target/release/vntext auto ryujinx --web --translate
```

- Mở game tới chỗ có thoại rồi mới chạy.
- `ryujinx` là tên process (khớp một phần, không phân biệt hoa thường); cũng có thể đưa PID.
- `--web` mở trang `http://127.0.0.1:8777`: mỗi đoạn thoại hiện lên cùng bản dịch, rê chuột để tra từ bằng Yomitan.
- `--translate` cần biến môi trường `GEMINI_API_KEY`. Mặc định dịch sang tiếng Việt (`--to en` để đổi) bằng model Flash-Lite mới nhất mà API liệt kê (`--model` để chỉ định).

Các lệnh khác:

| Lệnh | Tác dụng |
|---|---|
| `auto <pid\|tên>` | Theo dõi và in đoạn thoại mới. Tự nhận engine; không nhận ra thì quét chung |
| `strings <pid\|tên>` | In mọi chuỗi tiếng Nhật đang có trong bộ nhớ. Dùng để khảo sát game mới |
| `find <pid\|tên> <chuỗi>` | Tìm địa chỉ một chuỗi đã biết, ở cả ba bảng mã |
| `watch <pid\|tên> <địa chỉ hex> <utf8\|utf16\|sjis>` | Theo dõi một địa chỉ cố định |

Tuỳ chọn chung: `--clip` (đưa vào clipboard qua `wl-copy`), `--all` (quét cả vùng chỉ đọc), `--min N` (độ dài chuỗi tối thiểu). Khi có profile engine: `--raw` in text nguyên dạng engine lưu, gồm cả mã điều khiển.

## Cấu trúc

```
src/
  main.rs            Dòng lệnh và các vòng lặp chính
  mem.rs             Đọc bộ nhớ process khác
  text.rs            Tìm chuỗi tiếng Nhật trong dữ liệu thô
  engines/
    mod.rs           Giao diện chung của mọi profile, danh sách engine
    shin/
      mod.rs         Profile engine Entergram shin
      text.rs        Giải mã text của shin (bảng ký tự, lệnh @)
  translate.rs       Dịch qua Gemini API, có cache
  web.rs             Máy chủ trang web, đẩy sự kiện tới trình duyệt
  page.html          Trang hiển thị cho Yomitan
```

Tool gồm ba tầng. Chỉ tầng giữa phụ thuộc engine.

### Tầng 1: đọc bộ nhớ (`mem.rs`)

- Liệt kê vùng nhớ từ `/proc/<pid>/maps`, đọc qua `/proc/<pid>/mem`.
- Chỉ đọc trang đang có mặt trong RAM (xem qua `/proc/<pid>/pagemap`). Đọc một trang trống của vùng shm sẽ khiến kernel cấp phát nó, có thể ngốn hàng GB.
- Khử trùng lặp theo (file, vị trí trang). Ryujinx map cùng một file shm hàng nghìn lần; không khử thì quét 512 GB địa chỉ ảo thay vì khoảng 4 GB thật.
- Bỏ qua vùng thiết bị (`/dev/*` trừ shm) và vài vùng vô ích đã biết.
- Tốc độ quét khoảng 1 GB/giây trên một luồng.

### Tầng 2: tìm ra câu đang hiển thị

Có hai đường, `auto` tự chọn.

**Profile engine** (`engines/`) — đường chính xác. Hiện có một profile, shin:

1. Tìm file kịch bản trong RAM qua chữ ký `SNR `, đọc cả file.
2. Tách mọi lệnh thoại `MSGSET` (opcode `0x86`), kiểm tra bằng việc số thứ tự thoại phải liên tiếp. Bố cục lệnh lệch nhau giữa các game: bảng `LAYOUTS` liệt kê các kiểu đã biết (Umineko 7 byte đầu, Higurashi 8), tool thử hết và giữ kiểu đọc ra nhiều đoạn hơn.
3. Tìm ô nhớ vị trí của máy ảo script: một ô 32-bit chứa vị trí bắt đầu của một lệnh thoại, và 8 byte trước nó là vị trí ngay sau lệnh đó.
4. Đọc ô đó mỗi 50 ms; khi nó trỏ sang lệnh thoại khác thì in đoạn mới.
5. Nếu 15 giây không có đoạn mới, tìm lại ô vị trí ở luồng nền (ô này đổi chỗ khi game dựng lại máy ảo).

Text của shin là Shift-JIS, trong đó hiragana và vài dấu câu được ghi bằng katakana nửa độ rộng một byte, xen với lệnh `@` (`@r` xuống dòng, `@k` chờ bấm, `@v` voice, `@c` màu, `@b…@<…@>` furigana, ...). `engines/shin/text.rs` chuyển về text đọc được. Bảng ký tự và cú pháp lệnh lấy từ [DCNick3/shin](https://github.com/DCNick3/shin) (MPL-2.0).

**Quét chung** (`text.rs`, hàm `auto` trong `main.rs`) — đường dự phòng:

- Tìm chuỗi tiếng Nhật ở UTF-8, UTF-16LE và Shift-JIS, lọc dữ liệu nhị phân trông giống chữ (phải có kana liền nhau, kanji thuộc bảng Shift-JIS, không lặp vài ký tự, ...).
- Quét lặp lại toàn bộ process, coi địa chỉ nào đổi nội dung là ứng viên, chờ nội dung ổn định rồi in chuỗi dài nhất của mỗi đợt.

### Tầng 3: đưa text ra (`translate.rs`, `web.rs`, `page.html`)

- **Dịch**: mỗi đoạn thoại được dịch nguyên khối, kèm 4 đoạn trước làm ngữ cảnh. Một lần gọi API dịch luôn 8 đoạn kế tiếp để khi đọc tới đã có sẵn. Kết quả lưu ở `~/.cache/vntext/<ngôn ngữ>.tsv`.
- **Trang web**: máy chủ HTTP tối giản trên `127.0.0.1`, đẩy sự kiện bằng server-sent events, giữ 200 sự kiện gần nhất cho trình duyệt kết nối sau.

Dịch và trang web hiện chỉ nối với đường profile engine; chế độ quét chung chỉ in ra terminal và clipboard.

## Giới hạn

- **Quét chung chưa dùng được.** Trên Umineko nó chỉ bắt được mảnh của bộ nhớ đã giải phóng: lúc đủ, lúc cụt đầu, lúc mất hẳn. Mỗi lượt quét mất 3–6 giây.
- **Chưa bám theo từng lần bấm.** Một đoạn thoại gồm vài câu mà game hiện dần; tool in cả đoạn ngay khi game tới đó.
- **Tua nhanh có thể sót đoạn**, vì ô vị trí chỉ được đọc mỗi 50 ms.
- **Lựa chọn, menu, TIPS** chưa được kiểm tra.
- **Cần quyền đọc process**: cùng user và `kernel.yama.ptrace_scope = 0`.

## Thêm engine mới

Mỗi họ engine là một module trong `src/engines/`, cài đặt giao diện `Engine` (`engines/mod.rs`):

| Hàm | Việc |
|---|---|
| `detect(process)` | Nhận ra engine trong process và nạp kịch bản; trả về rỗng nếu không phải |
| `len()`, `passage(i)` | Số đoạn thoại, và đoạn thứ `i`: người nói, các câu đã giải mã, text gốc |
| `locate(process)` | Tìm nơi game lưu vị trí đang đọc. Chậm (quét toàn bộ), chỉ gọi lúc đầu và khi mất dấu |
| `current(process, vị trí)` | Đoạn đang hiển thị. Nhanh, gọi 20 lần mỗi giây |

Thêm một họ engine gồm hai việc: viết module mới, và thêm hàm `detect` của nó vào `DETECTORS` trong `engines/mod.rs`. Phần dịch, trang web và dòng lệnh không phải sửa.

Khác biệt giữa các game cùng họ nên để thành bảng dữ liệu trong module (như `LAYOUTS` của shin), không thành nhánh code.

Giao diện này mới được rút ra từ một engine duy nhất, nên nhiều khả năng phải chỉnh khi gặp engine thứ hai, nhất là nếu engine đó không có ô nhớ vị trí kiểu shin.

Quy trình khảo sát một game mới, theo kinh nghiệm từ shin:

1. **Tra nghiên cứu có sẵn trước.** GARbro (định dạng archive và script), Textractor và LunaTranslator (text nằm ở đâu, bảng mã gì), các tool dịch của cộng đồng, bản viết lại engine nếu có. Với shin, tự mò mất một vòng sai mà repo có sẵn đáp án.
2. Chạy `strings` lúc game đang hiện thoại để biết bảng mã và mã điều khiển; dùng `find` với một cụm trên màn hình để tìm bản gốc.
3. Xác định kịch bản nằm trong RAM hay phải giải nén từ file game.
4. Tìm cách biết câu đang hiển thị. Với shin là ô vị trí của máy ảo. Một cách chung hơn, chưa thử: dùng các mảnh text mà quét chung bắt được để tra ngược vào kịch bản đầy đủ.

Engine đã nhận diện được trong `/mnt/evo860/HGAME/` (theo file đặc trưng): CatSystem2 (loạt Grisaia, `*.int`), YU-RIS (`pac/*.ypf`), Unity (`UnityPlayer.dll`). Các thư mục còn lại chưa xác định.

## Nguồn tham khảo

- [DCNick3/shin](https://github.com/DCNick3/shin): bản viết lại engine Entergram; nguồn của định dạng SNR, bảng ký tự và cú pháp lệnh.
- [0xDC00/agent](https://github.com/0xDC00/agent): text hooker dùng Frida, điểm xuất phát của dự án.
