# Plan: vntext thân thiện cho người mới

Tài liệu giao việc cho người (hoặc model) implement. Đọc hết mục 1 đến 3 trước
khi đụng code. Mỗi giai đoạn ở mục 4 tự đứng được: làm xong, dừng, chờ chủ dự án
test trong game thật rồi mới sang giai đoạn sau.

## 1. Mục tiêu

vntext hiện chạy tốt cho chủ dự án, người quen terminal. Sắp tới tool được chia
sẻ cho bạn bè (không public). Mục tiêu của plan: **một người dùng KDE, không
biết terminal, mở tool lên là đọc được game**: chọn game bằng chuột, tra từ
được mà không cần Firefox, lỗi nào cũng có lời giải thích và cách sửa.

Không nằm trong plan (chủ dự án đã bỏ, đừng làm): phím tắt qua `vntext ctl`,
tùy chọn độ trễ OCR, chữ vô hình đè lên chữ game, quét bộ nhớ chung cho game
chưa có profile, menu của profile `shin`, viết lại giao diện bằng toolkit khác.

## 2. Luật làm việc

- **Không tự mở game, không bấm chuột/gõ phím giả (xdotool) trên desktop.** Chủ
  dự án có thể đang làm việc qua RustDesk. Cần game chạy thì hỏi.
- **Chỉ commit khi chủ dự án nói "commit đi".** Xong một giai đoạn: build sạch,
  test qua, báo lại những gì cần test tay, rồi chờ.
- Không dùng key Gemini của chủ dự án để tự thử. Không đổi cấu hình chung của
  Faugus (ví dụ bật Wayland driver) khi chưa được phép.
- Không in hàng loạt text của game ra log hay test. Fixture trong test tự viết.
- Kill tiến trình thì theo đúng pid, không `pkill` theo tên.
- Trả lời chủ dự án bằng tiếng Việt, xưng hô thân mật (tao/mày).
- Chữ hiện trên giao diện: tiếng Việt. Code, comment, thông báo ở terminal:
  tiếng Anh, theo đúng giọng comment đang có trong repo (câu ngắn, nói lý do).
- Giữ kiểu code hiện tại: hàm nhỏ, tên một từ khi được, hằng số có comment một
  dòng, test đặt cuối file trong `mod tests`. Không thêm lớp trừu tượng khi
  chỉ có một nơi dùng.
- Bản quyền không phải rào cản: được chép code GPL (Yomitan, Textractor…).
- Trước khi tự mò một định dạng hay giao thức, tìm repo/tài liệu có sẵn.
- README (tiếng Việt) phải được sửa cùng lúc với tính năng.

## 3. Hiện trạng cần biết

Repo `~/Projects/vntext`, Rust, chạy bằng `./target/release/vntext` (không nằm
trên PATH). 53 test, `cargo test --release`.

| File | Việc |
|---|---|
| `src/main.rs` | Lệnh `auto`, `ocr`, `strings`, `find`, `watch`; `parse_args` → `Opts`; `follow()` là vòng đọc theo profile; `translator_worker`. Lỗi thì `die()` thoát tiến trình. |
| `src/engines/` | Trait `Engine`, danh sách `DETECTORS`, `detect(&Proc)`. 10 profile. **Đây là đường chính của tool, không đụng tới trong plan này.** |
| `src/mem.rs` | Đọc `/proc/<pid>/mem`; `resolve(target)` (pid hoặc tên), `name(pid)` (tên tiến trình, dùng làm khóa lưu cấu hình theo game). |
| `src/screen.rs` | X11 qua `x11rb`: `Screen::open`, `window_of(pid)`, `placement`, `active_pid`, `pointer`, `grab` → `Picture` (ảnh xám 16 mức), PNG tự mã hóa. |
| `src/ocr.rs` | Đọc ô thoại bằng ảnh gửi Gemini. `Settings`, `Control`, `spawn`, `run`, `config_dir(biến, thư mục)`, vùng lưu ở `~/.config/vntext/regions.tsv`. |
| `src/overlay.rs` | Dải nổi GTK4 + `gtk4-layer-shell`, chạy trong thread riêng (`overlay::start`). `Look` (lưu theo game ở `~/.config/vntext/overlay.json`), `Shown`, `Ui`, `Overlay`; `set_up()` là hộp cài đặt dịch; `look_up()` tra từ dưới con trỏ; `tick()` 100 ms làm mọi việc định kỳ. |
| `src/yomitan.rs` | Hỏi Yomitan API `127.0.0.1:19633/termEntries`; `lookup(text) -> Result<Found, String>`; `Found{length, entries}`, `Entry`, `Definition`, `Block`; `Writer` đổi structured content của từ điển sang Pango markup. |
| `src/translate.rs` | Các nguồn dịch (`Service`, bảng `SERVICES`, `setup`, `chosen`, `save_setup`, `choose`, `at_work`), lưu ở `~/.config/vntext/translate.json`. Google không cần key. |
| `src/dict/` | Bảng luật chia từ của Yomitan và máy chia ngược (xem giai đoạn 2, bước 1). Chưa có kho từ điển. |
| `src/web.rs` | `Hub`: trang web `--web` và kênh sự kiện (`message`, `translation`, `notice`) mà dải nổi cũng nghe qua `subscribe()`. |

Điều kiện chạy hiện tại: phiên Wayland có layer-shell (KDE, wlroots; GNOME
không có), cửa sổ game là X11 (game Wine/Proton qua XWayland), đọc được bộ nhớ
tiến trình khác (`kernel.yama.ptrace_scope = 0` trên máy chủ dự án).

Máy chủ dự án: CachyOS, KDE Plasma, KWin 6.7.5, Node có sẵn, Yomitan trên
Firefox.

## 4. Các giai đoạn, theo thứ tự làm

### Giai đoạn 1: mở tool không cần gõ lệnh

**Kết quả:** chạy `vntext` không tham số (hoặc bấm biểu tượng trong menu ứng
dụng) hiện cửa sổ chọn game; bấm một game là dải nổi hiện lên.

1. **Cửa sổ chọn game** (`src/launcher.rs`, cửa sổ GTK thường, không phải
   layer-shell). `main()` gọi nó khi không có tham số; `vntext --help` vẫn in
   `USAGE`.
   - Danh sách ứng viên: các cửa sổ X11 đang mở (thêm vào `Screen` một hàm liệt
     kê `_NET_CLIENT_LIST` kèm pid và tiêu đề). Trên phiên Wayland, cửa sổ X11
     gần như chỉ còn game, nên danh sách tự sạch. Mỗi dòng: tiêu đề cửa sổ, tên
     tiến trình.
   - Nhận engine chạy ở thread phụ cho từng dòng (`Proc::open` +
     `engines::detect`, có thể mất vài giây): dòng hiện "đang nhận…" rồi tên
     engine hoặc "chưa có profile". Danh sách làm mới mỗi vài giây, không nhận
     lại game đã nhận.
   - Tùy chọn cho game đang chọn: Dịch (bật/tắt), ngôn ngữ đích, nguồn dịch
     (từ `translate::SERVICES`), trang web cho Yomitan (bật/tắt).
   - Nút chính: "Đọc game này" nếu có profile; "Đọc bằng OCR" nếu không, kèm
     một dòng giải thích OCR cần key Gemini và sẽ hỏi khoanh ô thoại.
2. **Cách khởi động phần đọc:** cửa sổ chọn game **chạy lại chính nó như tiến
   trình con** (`std::env::current_exe()`), ví dụ
   `vntext auto <pid> --overlay --translate --via google` hoặc
   `vntext ocr <pid> --overlay --translate`. Lý do: `follow()` và `ocr::run()`
   chặn mãi và thoát bằng `die()`; gọi qua tiến trình con thì không phải sửa
   chúng. Cửa sổ ẩn đi khi con chạy; con thoát thì hiện lại, kèm dòng cuối
   stderr của con nếu nó thoát vì lỗi (đi qua bộ diễn giải lỗi ở giai đoạn 4
   khi có).
3. **Nhớ theo game:** `~/.config/vntext/games.json`, khóa là `mem::name(pid)`
   (cùng khóa với `regions.tsv` và `overlay.json`): dịch hay không, ngôn ngữ,
   nguồn dịch, web. Game đã từng mở thì các tùy chọn tự điền.
4. **`vntext install`:** ghi `~/.local/share/applications/vntext.desktop` với
   `Exec=` là đường dẫn tuyệt đối của file đang chạy, `Name=vntext`,
   `Categories=Game;Utility;`, và dòng
   `X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2` (giai đoạn 5
   cần). Cửa sổ chọn game có nút "Thêm vào menu ứng dụng" gọi cùng hàm đó.

**Xong khi:** mở từ menu ứng dụng, chọn một game có profile và một game không
có profile đều lên dải nổi; đóng game thì cửa sổ chọn game hiện lại; mở lại
game cũ thấy tùy chọn cũ. Các lệnh dòng lệnh cũ chạy y như trước.

### Giai đoạn 2: tra từ có sẵn trong tool

**Kết quả:** rê chuột lên chữ Nhật trên dải nổi là ra nghĩa, không cần
Firefox. Người dùng tự tải từ điển Yomitan (file zip) và thả vào tool.

**Quyết định đã chốt:**
- Không nhúng máy JS, không chép cả Yomitan. Lấy **bảng luật chia từ của
  Yomitan nguyên xi dưới dạng dữ liệu**, viết lại phần máy bằng Rust (phần
  này đã làm xong, xem bước 1).
- Tool **không tự tải** từ điển nào. Giao diện có nút mở trang
  <https://yomitan.wiki/dictionaries/> để người dùng tự chọn bản hợp với mình.
- Đường Yomitan qua Firefox giữ lại như một lựa chọn.

1. **Bảng luật và máy chia ngược: ĐÃ XONG, đừng viết lại.** Nằm ở
   `src/dict/`, đã đối chiếu với chính Yomitan 26.9.8.0 trên toàn bộ mẫu thử:
   - `tools/yomitan-dump.mjs` xuất luật và bảng ký tự của Yomitan ra
     `src/dict/japanese.json`, và xuất kết quả của chính Yomitan trên một loạt
     chữ mẫu ra `src/dict/vectors.json` để test so. Cập nhật theo bản Yomitan
     mới thì chạy lại script (cách chạy ghi ở đầu file).
   - `src/dict/deinflect.rs`: `Transformer` (bản Rust của
     `LanguageTransformer`).
   - `src/dict/variants.rs`: các cách viết khác của một đoạn chữ (nửa rộng,
     dấu kết hợp, hiragana/katakana, rút chuỗi nhấn mạnh, kanji dị thể…).
   - `src/dict/mod.rs`: thứ phần tra cần gọi:
     - `dict::candidates(source) -> Vec<Candidate>`: mọi chuỗi mà từ điển có
       thể liệt kê cho `source` (coi `source` là trọn một từ). Mỗi
       `Candidate` có `text`, `conditions`, `rules` (các phép chia đã gỡ).
     - `Candidate::listed_as(marks) -> bool`: dòng từ điển có cột `rules` là
       `marks` (ví dụ `"v5 vt"`, hoặc rỗng) có đúng là từ mà ứng viên này
       được suy ra không. **Phải gọi hàm này cho mọi dòng tìm được**, nếu
       không sẽ ra kết quả sai kiểu 食べた → một động từ v5 nào đó.
   - `mod dict` trong `main.rs` đang mang `#[allow(dead_code)]`; gỡ dòng đó
     khi phần tra dùng tới nó.
   - Đủ cả 9 bộ tiền xử lý của Yomitan, kể cả bộ đọc chữ Latin thành kana
     (Shinjuku → しんじゅく), để tra được tên người và địa danh viết romaji.
   - Test so với Yomitan trên khoảng 8.600 chuỗi chia từ (phủ đủ 889 luật)
     và 10.900 chuỗi biến thể; `SAMPLES=10 SEED=1` trước lệnh chạy script để
     thử với bộ mẫu lớn hơn và khác đi.
2. **Kho từ điển** (`src/dict/store.rs`): SQLite (`rusqlite`, feature
   `bundled`) ở `~/.local/share/vntext/dict.sqlite` (dùng
   `ocr::config_dir("XDG_DATA_HOME", ".local/share")`). Bảng: `dictionaries`
   (tên, bản, thứ tự ưu tiên, bật/tắt), `terms` (chỉ mục theo `term` và theo
   `reading`), `meta` (chỉ mục theo `term`), `tags`.
3. **Nhập zip** (`src/dict/import.rs`, crate `zip`): theo schema trong
   `ext/data/schemas/` của Yomitan.
   - `index.json`: tên, bản, `format`/`version` (chỉ nhận 3; bản 1 thì báo
     không hỗ trợ).
   - `term_bank_N.json`: mỗi dòng
     `[term, reading, definitionTags, rules, score, glossary, sequence, termTags]`.
     Phần tử của `glossary`: chuỗi, `{type:"text"}`,
     `{type:"structured-content"}`, `{type:"image"}` (bỏ qua), hoặc mảng
     `[gốc, [luật]]` (bỏ qua ở bản đầu). Lưu `glossary` dạng JSON nguyên.
   - `term_meta_bank_N.json`: `[term, "freq"|"pitch"|"ipa", data]`; chỉ dùng
     `freq`, với các dạng số, chuỗi, `{value, displayValue}`,
     `{reading, frequency}`.
   - `tag_bank_N.json`: `[name, category, order, notes, score]`.
   - Nhập trong một transaction, báo tiến độ theo số file đã xong, hủy được.
     Nhập trùng tên thì thay bản cũ.
4. **Tra** (`src/dict/lookup.rs`, hàm cùng chữ ký với `yomitan::lookup`): với
   đoạn chữ từ con trỏ (tối đa `LOOKED_UP` ký tự), thử từ dài nhất xuống một
   ký tự; mỗi độ dài: `dict::candidates(đoạn)` → tìm trong kho theo `term`
   hoặc `reading` bằng `text` của từng ứng viên → giữ dòng mà
   `candidate.listed_as(cột rules của dòng)` trả true. Sắp xếp:
   đoạn gốc dài hơn trước, ít bước chia hơn, thứ tự ưu tiên từ điển, `score`
   cao hơn. Gộp theo (term, reading) thành `Entry`, mỗi từ điển một
   `Definition`, tần suất lấy từ `meta`. `Found.length` là số byte của đoạn
   gốc dài nhất có kết quả. Giữ giới hạn `ENTRIES`.
5. **Dùng lại phần hiển thị:** `yomitan.rs` đã có `Writer` cho structured
   content và các kiểu `Entry`/`Definition`/`Block` mà popup vẽ. Tách phần
   dựng `Definition` từ glossary thành hàm dùng chung cho cả hai nguồn; đừng
   viết bộ vẽ thứ hai. Popup trong `overlay.rs` (`word()`, `look_up()`) không
   đổi cách vẽ.
6. **Giao diện** (tab "Từ điển" trong hộp cài đặt, xem giai đoạn 4 về hộp cài
   đặt chung; nếu giai đoạn 4 chưa làm thì thêm tab vào `set_up()` hiện tại):
   - Danh sách từ điển đã nhập: công tắc bật/tắt, lên/xuống thứ tự, xóa.
   - Nút "Thêm từ điển…" mở hộp chọn file `.zip` (chọn nhiều được) và nhận
     file kéo thả vào danh sách; thanh tiến độ khi nhập.
   - Nút "Tải từ điển" mở <https://yomitan.wiki/dictionaries/> bằng trình
     duyệt mặc định, kèm một dòng: tải file zip về rồi thêm vào đây, không
     cần giải nén.
   - Chọn nguồn tra: "Từ điển trong tool" hoặc "Yomitan (Firefox)". Mặc định:
     có từ điển đã nhập thì dùng trong tool; chưa có mà Yomitan API trả lời
     thì dùng Yomitan; chưa có cả hai thì popup hiện "Chưa có từ điển" và nút
     mở tab này.
7. **Test:** nhập một zip tí hon dựng ngay trong test; tra chọn đoạn dài
   nhất; dòng sai loại từ bị loại; các dạng `freq`. (Chia ngược đã có test.)

**Rủi ro:** hộp cài đặt là mặt layer-shell giữ bàn phím độc quyền, hộp chọn
file mở từ đó có thể không lên trước hoặc không nhận phím. Thử sớm; nếu hỏng
thì hạ chế độ bàn phím trong lúc hộp chọn file mở, và luôn giữ đường kéo thả.

**Xong khi:** tắt Firefox, nhập một từ điển nghĩa và một từ điển tần suất,
rê chuột lên động từ đã chia ra đúng dạng gốc với popup trông như hiện tại;
chuyển sang "Yomitan (Firefox)" vẫn chạy như cũ.

### Giai đoạn 3: chạy được trên máy người khác

**Kết quả:** máy thiếu gì, tool nói rõ thiếu gì và sửa thế nào, thay vì im
lặng hoặc in lỗi tiếng Anh ra terminal.

1. **Tự kiểm tra** (`src/doctor.rs`; chạy khi mở cửa sổ chọn game và qua lệnh
   `vntext doctor`). Mỗi mục: đạt / không đạt, một câu giải thích, cách sửa.
   - Phiên Wayland có layer-shell (`gtk4_layer_shell::is_supported()`); nếu
     là GNOME thì nói thẳng dải nổi không chạy được ở đó.
   - Tới được máy chủ X (XWayland).
   - Đọc được bộ nhớ tiến trình khác: đọc
     `/proc/sys/kernel/yama/ptrace_scope`; giá trị khác 0 là không đạt.
   - Có `wl-copy` (gói `wl-clipboard`) cho nút copy.
   - Nguồn tra từ: có từ điển đã nhập hoặc Yomitan API trả lời.
   - Nguồn dịch: có key nào đã lưu không (không có vẫn đạt, vì có Google).
2. **Quyền đọc bộ nhớ:** nhiều distro đặt `ptrace_scope = 1`, tool sẽ bị từ
   chối khi mở `/proc/<pid>/mem`. Mục kiểm tra có nút "Cho phép", bấm thì hiện
   lời giải thích (thiết lập này cho mọi chương trình của cùng người dùng đọc
   bộ nhớ của nhau; là mặc định trên Arch) rồi chạy qua `pkexec`: ghi
   `kernel.yama.ptrace_scope = 0` vào `/etc/sysctl.d/90-vntext-ptrace.conf` và
   `sysctl -w` ngay. **Không dùng `setcap` cho file chạy:** file có capability
   bị coi là chương trình đặc quyền, GLib hạn chế nhiều thứ, và mỗi lần build
   lại phải cấp lại. Khi `Proc::open` vẫn bị từ chối, thông
   báo lỗi dẫn về đúng mục này.
3. **Mặc định cho người chưa có key:** cửa sổ chọn game bật Dịch với Google
   khi chưa nguồn nào có key. Không đổi hành vi của ai đã chọn nguồn khác.
4. **Chia sẻ bằng source, không đóng gói.** Chủ dự án đã chốt: đưa repo cho
   bạn bè tự build, không làm gói Arch, không làm AppImage. Việc cần làm là
   cho việc build dễ:
   - README có mục "Cài cho người mới": cài Rust và các gói hệ thống (`gtk4`,
     `gtk4-layer-shell`, `wl-clipboard`, kèm tên gói trên Arch và trên
     Debian/Ubuntu), `cargo build --release`, rồi
     `./target/release/vntext install` để có trong menu ứng dụng. Từ đó mở
     bằng chuột.
   - `vntext install` (giai đoạn 1) cũng đặt một liên kết ở
     `~/.local/bin/vntext` nếu thư mục đó có trên PATH.
   - Bảng "tool báo thiếu X thì làm Y" khớp với các mục kiểm tra.
   - Repo không được chứa key, cache dịch hay cấu hình riêng của chủ dự án;
     kiểm tra `.gitignore` và lịch sử trước khi chia sẻ, báo lại nếu thấy.

**Xong khi:** `vntext doctor` trên máy chủ dự án đạt hết; tạm đặt
`ptrace_scope = 1` (chủ dự án tự làm) thì cửa sổ chọn game báo đúng và nút
"Cho phép" sửa được; làm theo đúng mục "Cài cho người mới" từ một bản clone
mới thì build được và mở được từ menu.

### Giai đoạn 4: dọn dải nổi

**Kết quả:** hàng nút ngắn lại, mọi tùy chỉnh ở một chỗ, lỗi nói bằng lời
thường. Trước khi sửa, đọc `run()` và các lời gọi `self.button(...)` trong
`overlay.rs` để lập danh sách nút đang có.

1. **Một hộp cài đặt, nhiều tab** (mở rộng `set_up()`): "Hiển thị" (cỡ chữ,
   độ mờ nền, màu chữ, chế độ Dịch/Nhật/cả hai, nút đặt lại vị trí), "Dịch"
   (nội dung hiện tại của `set_up()`), "Từ điển" (giai đoạn 2), "OCR" (khoanh
   lại ô thoại, bật/tắt đọc bằng ảnh). Hàng nút chỉ còn những thứ bấm thường
   xuyên: ẩn/hiện, khóa, copy, đổi chế độ, OCR, và bánh răng mở hộp cài đặt.
   Nút nào bị dời vào hộp thì bỏ khỏi hàng, không để hai nơi.
2. **Độ mờ và màu:** thêm vào `Look` hai trường `opacity` (0.2 đến 1.0) và
   `colour`; `from_json` phải đọc được file cũ thiếu hai trường này. Áp bằng
   một `CssProvider` riêng của dải, thay cho màu nền viết cứng trong `STYLE`.
3. **Xem lại câu cũ:** `Shown::step(by)` đã có. Nối bánh xe chuột trên dải vào
   nó nếu chưa nối, hiện "3/40" khi đang xem câu cũ, có câu mới thì về câu
   mới nhất.
4. **Chỉ dẫn lần đầu:** lần đầu dải hiện trên máy, thay cho câu thoại là ba
   dòng: rê chuột lên chữ Nhật để tra, bánh răng để chỉnh, kéo tay nắm để dời.
   Có nút "Đã hiểu"; đánh dấu đã xem trong `overlay.json`.
5. **Lỗi bằng lời thường** (`src/explain.rs`): một hàm nhận chuỗi lỗi thô, trả
   câu tiếng Việt và hành động gợi ý. Tối thiểu: hết hạn mức (429) → đề nghị
   chuyển sang Google, kèm nút; key sai (401/403) → mở tab Dịch; mất mạng/hết
   giờ; Yomitan không trả lời → mở tab Từ điển; game không có cửa sổ X11; bị
   từ chối đọc bộ nhớ → mục quyền ở giai đoạn 3. Dùng cho notice của dịch, của
   OCR và của cửa sổ chọn game. Lỗi thô vẫn in ra stderr.

**Xong khi:** hàng nút còn tối đa sáu nút; đổi độ mờ và màu thấy ngay và còn
nguyên sau khi mở lại; file `overlay.json` cũ vẫn đọc được; rút mạng lúc đang
dịch thì dải hiện câu tiếng Việt, không phải chuỗi lỗi HTTP.

### Giai đoạn 5: backend KWin cho cửa sổ Wayland

**Kết quả:** game chạy bằng Wayland driver của Wine (hoặc ứng dụng Wayland
thuần) vẫn có dải nổi bám theo và OCR chụp được, trên KDE.

Dải nổi vốn đã là Wayland. Thứ dính X11 chỉ là `screen.rs`: tìm cửa sổ của
pid, vị trí và kích thước, cửa sổ nào đang được focus, chụp ô thoại. Wayland
không có giao thức chuẩn cho những việc này nên phải hỏi KWin.

1. **Tách `screen.rs`:** đổi `Screen` thành trait với các hàm đang có
   (`window_of`, `placement`, `active_pid`, `pointer`, `grab`), mã cửa sổ thành
   kiểu riêng (số X11 hoặc UUID của KWin). Hai bản: `X11` (code hiện tại) và
   `Kwin`. Một bản ghép hỏi X11 trước, không thấy cửa sổ của pid thì hỏi KWin
   (khi `XDG_CURRENT_DESKTOP` có `KDE`). Biến môi trường `VNTEXT_SCREEN=kwin`
   ép dùng KWin, để thử với game X11 mà không phải đổi cấu hình Faugus.
2. **Vị trí và focus qua KWin script.** D-Bus đã xác nhận trên máy chủ dự án:
   `org.kde.KWin`, đối tượng `/Scripting`, interface `org.kde.kwin.Scripting`
   có `loadScript(filePath, pluginName) -> int`, `unloadScript(pluginName)`,
   `isScriptLoaded(pluginName)`. Sau `loadScript` phải gọi `run` trên đối
   tượng script vừa tạo (xem bằng `qdbus6 org.kde.KWin /Scripting/Script<id>`).
   - vntext ghi script JS vào `$XDG_RUNTIME_DIR`, nạp với tên riêng theo pid
     của mình, gỡ khi thoát; lúc khởi động gỡ bản cũ còn sót.
   - Script theo dõi các cửa sổ có `pid` trùng mục tiêu và gọi ngược về vntext
     bằng `callDBus(...)` mỗi khi: cửa sổ xuất hiện/biến mất, đổi vị trí hay
     kích thước, thu nhỏ, đổi cửa sổ đang focus. Gửi: `internalId`,
     `clientGeometry` (vùng nội dung, không tính viền, để khớp với vùng OCR đã
     lưu theo tỷ lệ), có đang focus không.
   - vntext giữ một tên trên session bus để nhận các lời gọi đó (crate `zbus`,
     bản blocking) và lưu trạng thái mới nhất; `placement`/`active_pid` chỉ đọc
     trạng thái này, không hỏi D-Bus mỗi tick.
   - `pointer` chỉ được dùng bởi cách khoanh vùng trong terminal
     (`ocr::pick`). Bản KWin trả `None` và lệnh `ocr` không kèm `--overlay`
     báo: với cửa sổ Wayland hãy khoanh vùng trên dải nổi.
3. **Chụp qua `org.kde.KWin.ScreenShot2`** (đối tượng
   `/org/kde/KWin/ScreenShot2`), hàm `CaptureWindow(handle, options, pipe)`:
   `handle` là `internalId`, `options` tắt con trỏ, viền và bóng, bật độ phân
   giải gốc; KWin trả kích thước, `stride`, `format` rồi ghi điểm ảnh thô vào
   pipe. Đọc hết pipe, cắt đúng vùng ô thoại, đổi sang xám, đưa vào
   `Picture::from_grey`. Dùng `CaptureWindow` chứ không dùng `CaptureArea`:
   chụp theo vùng màn hình sẽ dính luôn dải nổi đang đè lên game.
   - Quyền: KWin chỉ cho chương trình có file `.desktop` khai
     `X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2` với `Exec`
     trỏ đúng file chạy. `vntext install` (giai đoạn 1) đã ghi dòng này. Bị từ
     chối thì báo người dùng bấm "Thêm vào menu ứng dụng". **Xác nhận cơ chế
     này trên KWin 6.7.5 trước khi viết phần còn lại.**
   - Mỗi lần chụp là cả cửa sổ, nặng hơn `GetImage` một vùng nhỏ. Đo CPU; nếu
     cao thì nới nhịp chụp riêng cho bản KWin (150 ms → khoảng 300 ms).
4. **Chỗ chưa biết, phải xác nhận bằng thử thật:** với Wayland driver của
   Wine, `pid` mà KWin gán cho cửa sổ có trùng pid tiến trình game mà vntext
   đang đọc bộ nhớ không. Nếu lệch, khớp theo cây tiến trình (cùng wineserver)
   thay vì đúng một pid.
5. **Kiểm tra môi trường:** thêm vào `doctor` mục "cửa sổ game là Wayland và
   desktop không phải KDE → chưa hỗ trợ, hãy tắt Wayland driver của Wine".
   README sửa phần điều kiện chạy.

**Thử:** trước hết với game X11 và `VNTEXT_SCREEN=kwin` (KWin thấy cả cửa sổ
XWayland). Thử với Wayland driver thật thì **hỏi chủ dự án**, vì phải đổi cấu
hình Faugus và mở game.

**Xong khi:** với `VNTEXT_SCREEN=kwin`, dải bám theo cửa sổ khi kéo và đổi cỡ,
ẩn khi chuyển sang cửa sổ khác, OCR đọc được; không còn script KWin nào sót
lại sau khi thoát (kể cả thoát bằng Ctrl+C); đường X11 chạy y như trước.

## 5. Sau mỗi giai đoạn

1. `cargo build --release` không cảnh báo, `cargo test --release` qua hết.
2. README đã sửa cho khớp.
3. Báo cho chủ dự án: đã làm gì, cần test tay những gì, chỗ nào chưa kiểm
   chứng được. Chưa test thì nói là chưa test.
4. Chờ "commit đi".
