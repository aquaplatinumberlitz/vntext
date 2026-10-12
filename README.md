# vntext

Lấy text tiếng Nhật ra khỏi visual novel đang chạy để đọc cùng Yomitan và bản dịch máy.

Tool đọc bộ nhớ của game từ bên ngoài (`/proc/<pid>/mem`), không inject và không hook code, nên không phụ thuộc phiên bản emulator. Dự án bắt đầu vì hook kiểu Frida (agent của 0xDC00) gãy trên Ryujinx 1.3.3.

Chỉ chạy trên Linux. Dùng cá nhân, chưa công bố.

## Tình trạng

| Game | Engine | Nền tảng | Kết quả |
|---|---|---|---|
| Umineko no Naku Koro ni Saku | Entergram shin | Switch, Ryujinx 1.3.3 | Chính xác từng đoạn, đã kiểm tra |
| Higurashi no Naku Koro ni Hou | Entergram shin | Switch, Ryujinx 1.3.3 | Chính xác từng đoạn, đã kiểm tra |
| 牝教師 1, 2 | BISHOP (BSXScript 3.0) | Windows, Proton-GE qua Faugus | Chính xác từng câu, đã kiểm tra |
| 牝教師 4, Sansha Mendan (bản tiếng Anh) | BISHOP (BSXScript 3.1) | Windows, Proton-GE qua Faugus | Chính xác từng câu, đã kiểm tra |
| 家属～母と姉妹の嬌声, Shihai no Kyoudan (bản tiếng Anh) | BISHOP (BSXScript 3.2) | Windows, Proton-GE qua Faugus | Chính xác từng câu, đã kiểm tra |
| 牝教師 3, 淫獄の放課後 1–2, 隷従の制服, Shihai no Kyoudan 2 (bản tiếng Anh) | BISHOP (BSXScript 3.3) | Windows, Proton-GE qua Faugus | Chính xác từng câu, đã kiểm tra |
| 催眠術2, 催眠術2 URA | BLACKRAINBOW spt | Windows, Proton-GE qua Faugus | Chính xác từng câu, đã kiểm tra |
| 催眠術3, 催眠術4, 催眠術 RE | MED | Windows, Proton-GE qua Faugus | Chính xác từng khung thoại, đã kiểm tra |
| Game engine khác | chưa có profile | bất kỳ | Chỉ có chế độ quét chung, chưa dùng được (xem [Giới hạn](#giới-hạn)) |

## Dùng

```
cargo build --release
./target/release/vntext auto ryujinx --web --translate
```

- Mở game tới chỗ có thoại rồi mới chạy.
- `ryujinx` là tên process (khớp một phần, không phân biệt hoa thường); cũng có thể đưa PID. Process có tên khớp được ưu tiên hơn process chỉ có chuỗi đó trong dòng lệnh, vì game chạy qua launcher có các lớp bọc (umu-run, proton) mang đường dẫn game làm tham số.
- `--web` mở trang `http://127.0.0.1:8777`: mỗi đoạn thoại hiện lên cùng bản dịch, rê chuột để tra từ bằng Yomitan.
- `--translate` cần biến môi trường `GEMINI_API_KEY`. Mặc định dịch sang tiếng Việt (`--to en` để đổi) bằng model Flash-Lite mới nhất mà API liệt kê (`--model` để chỉ định).

Các lệnh khác:

| Lệnh | Tác dụng |
|---|---|
| `auto <pid\|tên>` | Theo dõi và in đoạn thoại mới. Tự nhận engine; không nhận ra thì quét chung |
| `strings <pid\|tên>` | In mọi chuỗi tiếng Nhật đang có trong bộ nhớ. Dùng để khảo sát game mới |
| `find <pid\|tên> <chuỗi>` | Tìm địa chỉ một chuỗi đã biết, ở cả ba bảng mã |
| `watch <pid\|tên> <địa chỉ hex> <utf8\|utf16\|sjis>` | Theo dõi một địa chỉ cố định |

Tuỳ chọn chung: `--clip` (đưa vào clipboard qua `wl-copy`), `--all` (quét cả vùng chỉ đọc), `--min N` (độ dài chuỗi tối thiểu). Khi có profile engine: `--raw` in text nguyên dạng engine lưu, gồm cả mã điều khiển; `--debug` in mọi lần một ô vị trí đổi giá trị, dùng khi tool hiện sai đoạn hoặc đứng im.

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
    bishop/
      mod.rs         Profile engine riêng của BISHOP (BSXScript)
      text.rs        Giải mã text của BISHOP (mã \n, \A, \B, ký tự riêng)
    med/
      mod.rs         Profile engine MED (催眠術3, 4, RE)
      text.rs        Dò khoá, giải mã script và text của MED
    spt/
      mod.rs         Profile engine cũ của BLACKRAINBOW (script .spt)
      text.rs        Giải mã text của spt
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

**Profile engine** (`engines/`) — đường chính xác. Profile shin:

1. Tìm file kịch bản trong RAM qua chữ ký `SNR `, đọc cả file.
2. Tách mọi lệnh thoại `MSGSET` (opcode `0x86`), kiểm tra bằng việc số thứ tự thoại phải liên tiếp. Bố cục lệnh lệch nhau giữa các game: bảng `LAYOUTS` liệt kê các kiểu đã biết (Umineko 7 byte đầu, Higurashi 8), tool thử hết và giữ kiểu đọc ra nhiều đoạn hơn.
3. Tìm ô nhớ vị trí của máy ảo script: một ô 32-bit chứa vị trí bắt đầu của một lệnh thoại, và 8 byte trước nó là vị trí ngay sau lệnh đó. Chỉ tìm trong vùng nhớ chứa kịch bản, tức RAM của game; bộ nhớ riêng của emulator cũng có dữ liệu khớp dạng này.
4. Đọc các ô tìm được mỗi 50 ms; khi ô trỏ sang lệnh thoại khác thì in đoạn mới.
5. Tìm lại vị trí ở luồng nền khi 3 giây không có đoạn mới (giãn dần tới 30 giây), và tìm liên tục khi chưa có gì để bám. Ô vị trí đổi chỗ khi load save hoặc về màn hình chính.

Không phải ô nào khớp dạng cũng là ô thật. Game giữ các bản sao trạng thái máy ảo (trạng thái của file save vừa load, bộ đệm đọc file save) trỏ tới một đoạn chẳng liên quan. Cách phân biệt:

- Ô nằm ở đầu trang nhớ, tại offset `0x10`, được tin ngay. Mọi ô thật quan sát được trên cả hai game đều nằm ở đó, bản sao thì không. Nhờ vậy lần tìm thông thường chỉ đọc vài byte đầu mỗi trang (dưới 1 giây thay vì khoảng 5 giây quét cả RAM).
- Ô ở chỗ khác chỉ được tin khi nó tiến từng bước nhỏ về phía trước hai lần liên tiếp, như một người đang đọc. Một phần mười số lần tìm là quét kỹ để thấy các ô loại này.

Text của shin là Shift-JIS, trong đó hiragana và vài dấu câu được ghi bằng katakana nửa độ rộng một byte, xen với lệnh `@` (`@r` xuống dòng, `@k` chờ bấm, `@v` voice, `@c` màu, `@b…@<…@>` furigana, ...). `engines/shin/text.rs` chuyển về text đọc được. Bảng ký tự và cú pháp lệnh lấy từ [DCNick3/shin](https://github.com/DCNick3/shin) (MPL-2.0).

Profile bishop (`engines/bishop/`), cho các game dùng `bsxx.dat`:

1. Tìm chữ ký `BSXScript 3.` trong RAM, rồi tìm ô nhớ trỏ tới nó; ô kế tiếp là kích thước script. Cặp ô đó nằm trong khối trạng thái của máy ảo, ở vị trí cố định suốt lần chạy.
2. Đọc script: bảng tên nhân vật, bảng câu thoại (UTF-16), và duyệt mã lệnh để lấy mọi lệnh "hiện câu thoại" (`0x1D`) theo thứ tự.
3. Trong khối trạng thái, ô vị trí là ô chứa điểm đầu của một lệnh hiện thoại, khi một ô gần đó chứa điểm cuối của chính lệnh ấy.
4. Giải mã text: `\n` là xuống dòng trong khung thoại (bỏ), `\b` và `\s` đổi cỡ chữ (bỏ), `\A` và `\B` là họ và tên nhân vật chính (đọc từ `bsxx.ini` trong thư mục game), `U+E000` là dấu ♥. Các ký tự `U+E0xx` khác được suy đoán là kana có dấu đục; chưa gặp trong game để đối chiếu.

Định dạng file và độ dài lệnh lấy từ [crskycode/BSXScript_Tool](https://github.com/crskycode/BSXScript_Tool).

Profile spt (`engines/spt/`), cho 催眠術2 và 催眠術2 URA:

1. Engine này không giữ file script trong RAM (nó biến mỗi script thành danh sách liên kết các lệnh), nên gói script `*_spt.dat` được đọc từ thư mục game.
2. Gói gồm nhiều file `.spt`; mỗi file là một dãy lệnh, lệnh `0x08` là câu thoại. Text là Shift-JIS, cứ 4 byte thì byte đầu bị đảo bit thấp nhất. Tên người nói nằm đầu câu dạng `【tên】`.
3. Vị trí đang đọc: chương trình giữ ở chỗ cố định phần đầu của script đang chạy (tên, số lệnh, kích thước), sau đó 12 byte là số thứ tự lệnh hiện tại, rồi tên script lặp lại một lần nữa. Máy ảo đứng yên ở lệnh thoại suốt lúc câu đó hiện trên màn hình; điều này đã được đo trên cả hai game.

Định dạng lấy từ một bộ công cụ patch tiếng Anh cho hai game này (`blackrb_spt.py`), không rõ tác giả.

Khi một profile cần file của game (gói script của spt, `bsxx.ini` của bishop), thư mục game được xác định bằng nơi chứa file exe đang được nạp vào process, rồi mới tới thư mục làm việc. Cách đầu không phụ thuộc launcher: dưới Wine, file exe hiện ra với đường dẫn thật trên Linux.

Profile med (`engines/med/`), cho 催眠術3, 催眠術4 và 催眠術 RE:

1. Gói script `md_scr.med` được đọc từ thư mục game. Mỗi file trong gói bị mã hoá bằng cách cộng từng byte với một khoá lặp lại; khoá được dò ra từ file `_VIEW`, vì 24 byte đầu của nó giống nhau ở mọi game dùng engine này.
2. Mỗi script là một dãy lệnh (số dòng nguồn, độ dài, mã lệnh, tham số) rồi tới bảng chuỗi; lệnh mang số thứ tự của chuỗi. Một khung thoại là một chuỗi lệnh: có thể có tên người nói (`13`), rồi text (`1`) và xuống dòng (`0`), kết thúc bằng lệnh xoá khung (`4`).
3. Vị trí đang đọc lấy từ trạng thái máy ảo trong RAM, gồm (tính theo ô 32-bit): độ dài phần mã lệnh của script đang chạy, cách 4 ô là bộ đếm chương trình, cách 4 ô nữa là bản sao của bộ đếm, và ngay sau đó là con trỏ tới tên script. Tool tìm cụm này bằng cách quét bộ nhớ, rồi mỗi lần đọc lại tên script và bộ đếm.
4. Bộ đếm trỏ tới lệnh kế tiếp; trong lúc một khung thoại hiện trên màn hình, máy ảo đứng ở lệnh xoá khung của nó.

Phải nhận diện script bằng tên: rất nhiều script trong cùng một game có phần mã lệnh mở đầu giống hệt nhau (ở 催眠術4, 339 script chỉ có 64 kiểu mở đầu khác nhau), nên so mã lệnh sẽ nhận nhầm.

Định dạng gói lấy từ GARbro; cách dò khoá và định dạng script lấy từ một bộ công cụ dịch cho engine này (`MED.py`, `SCE.py`), không rõ tác giả. `＄０` trong text là tên nhân vật chính và hiện chưa được thay.

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

Mỗi họ engine là một thư mục trong `src/engines/` với hai file: `mod.rs` (tìm kịch bản và theo dõi vị trí, cài đặt giao diện `Engine`) và `text.rs` (giải mã text của engine đó). Giao diện chung nằm ở `engines/mod.rs`:

| Hàm | Việc |
|---|---|
| `detect(process)` | Nhận ra engine trong process và nạp kịch bản; trả về rỗng nếu không phải |
| `len()`, `passage(i)` | Số đoạn thoại, và đoạn thứ `i`: người nói, các câu đã giải mã, text gốc |
| `locate(process, kỹ hay nhanh)` | Tìm các ô nhớ giữ vị trí đang đọc, kèm đánh dấu ô nào được engine nhận là ô thật. Gọi ở luồng nền |
| `read(process, địa chỉ ô)` | Đoạn mà một ô đang trỏ tới. Nhanh, gọi 20 lần mỗi giây cho mỗi ô |

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
- [crskycode/BSXScript_Tool](https://github.com/crskycode/BSXScript_Tool): công cụ xuất/nhập text của `bsxx.dat`; nguồn của định dạng BSXScript.
- [GARbro, ArcMED.cs](https://github.com/morkt/GARbro/blob/master/ArcFormats/DxLib/ArcMED.cs): định dạng gói `.med`.
- Bộ công cụ dịch cho engine MED (`MED.py`, `SCE.py`, không rõ tác giả): cách dò khoá và định dạng script.
- Bộ công cụ patch tiếng Anh cho 催眠術2 (`blackrb_spt.py`, không rõ tác giả): nguồn của định dạng gói `*_spt.dat` và file `.spt`.
- [0xDC00/agent](https://github.com/0xDC00/agent): text hooker dùng Frida, điểm xuất phát của dự án.
