# vntext
<img width="1338" height="934" alt="Screenshot_20261016_084456" src="https://github.com/user-attachments/assets/84075a7d-f99a-4463-84ab-943ce27ccdb4" />
<img width="1338" height="934" alt="Screenshot_20261016_084618" src="https://github.com/user-attachments/assets/438b1c26-3786-4bf3-88f9-1997f06550c6" />

Lấy text tiếng Nhật ra khỏi visual novel đang chạy trên Linux, hiện nó trong một dải nổi cạnh cửa sổ game, tra từ ngay trên đó và dịch máy cả đoạn.

- **Đọc thẳng từ game, không hook.** Tool đọc script từ file của game và chỉ nhìn bộ nhớ (`/proc/<pid>/mem`) để biết game đang đứng ở câu nào. Không inject gì vào game hay emulator, nên đổi phiên bản Wine, Proton hay Ryujinx không làm nó gãy.
- **Text đúng từng chữ, có tên người nói**, đã bỏ mã điều khiển và furigana, mỗi đoạn ra đúng một lần.
- **Dịch cả đoạn kèm ngữ cảnh**, và dịch sẵn các đoạn kế tiếp trong lúc bạn còn đọc đoạn hiện tại.
- **Tra từ kiểu Yomitan ngay trên dải nổi**, bằng từ điển Yomitan nhập vào tool, không cần trình duyệt.
- **Game chưa được hỗ trợ trực tiếp vẫn đọc được** bằng OCR: tool chụp khung thoại và nhờ Gemini đọc.

Dự án cá nhân, chỉ chạy trên Linux. Tài liệu viết bằng tiếng Việt; code và comment viết bằng tiếng Anh.

## Mục lục

- [Yêu cầu](#yêu-cầu)
- [Cài đặt](#cài-đặt)
- [Sử dụng](#sử-dụng)
- [Game đã hỗ trợ](#game-đã-hỗ-trợ)
- [Cấu trúc dự án](#cấu-trúc-dự-án)
- [Mở rộng engine khi gặp game mới](#mở-rộng-engine-khi-gặp-game-mới)
- [Giới hạn](#giới-hạn)
- [Port sang Windows (tham khảo)](#port-sang-windows-tham-khảo)
- [So với Textractor và agent](#so-với-textractor-và-agent)
- [Nguồn tham khảo](#nguồn-tham-khảo)

## Yêu cầu

| Cần | Ghi chú |
|---|---|
| Linux, phiên Wayland có layer-shell | KDE Plasma, Sway, Hyprland. GNOME không có layer-shell nên dải nổi không chạy ở đó; khi ấy chỉ dùng được dòng lệnh và trang web. |
| Game chạy dưới dạng cửa sổ X11 (XWayland) | Mặc định của Wine/Proton và Ryujinx. Game dùng Wayland driver của Wine chỉ được hỗ trợ trên KDE Plasma. |
| Quyền đọc bộ nhớ tiến trình khác | Cùng người dùng với game, và `kernel.yama.ptrace_scope = 0`. Tool có nút sửa giúp. |
| Rust | Để build từ source. |

## Cài đặt

Tool được chia sẻ bằng source: bạn tự build, không có gói cài sẵn.

1. **Cài Rust** (<https://rustup.rs>) và các gói hệ thống:

   | Cần | Arch | Debian/Ubuntu |
   |---|---|---|
   | GTK4 | `gtk4` | `libgtk-4-dev` |
   | layer-shell cho dải nổi | `gtk4-layer-shell` | `libgtk4-layer-shell-dev` |
   | nút chép | `wl-clipboard` | `wl-clipboard` |
   | trình biên dịch C (build SQLite đi kèm) | `base-devel` | `build-essential` |

2. **Lấy source và build:**

   ```
   git clone https://github.com/aquaplatinumberlitz/vntext.git
   cd vntext
   cargo build --release
   ```

3. **Đưa vào menu ứng dụng:**

   ```
   ./target/release/vntext install
   ```

   Lệnh này ghi mục `vntext` và icon vào menu ứng dụng, và đặt lệnh `vntext` ở `~/.local/bin` nếu thư mục đó có trên PATH. Chạy lại nó nếu bạn dời thư mục source đi chỗ khác.

4. **Mở vntext** từ menu ứng dụng. Nếu máy thiếu gì, đầu cửa sổ hiện khung báo thiếu gì và sửa thế nào. Cùng nội dung đó có ở lệnh `vntext doctor`.

Tool báo thiếu gì thì làm nấy:

| Tool báo | Làm |
|---|---|
| "Dải nổi": GNOME không có layer-shell | Dùng KDE Plasma, Sway hoặc Hyprland |
| "Dải nổi": không phải phiên Wayland | Đăng xuất, chọn phiên Wayland ở màn hình đăng nhập |
| "Cửa sổ X11 (XWayland)" | Kiểm tra `DISPLAY`/`XAUTHORITY` của phiên desktop; cài `xorg-xwayland` (Arch) hoặc `xwayland` (Debian/Ubuntu) nếu chưa có. Nếu socket phiên bị mất hoàn toàn, lưu công việc rồi đăng xuất/đăng nhập lại. |
| "Đọc bộ nhớ game", hoặc game ghi "bị từ chối đọc bộ nhớ" | Bấm "Cho phép" trong khung báo. Nó hỏi mật khẩu, ghi `/etc/sysctl.d/90-vntext-ptrace.conf` và áp dụng ngay. Thiết lập này cho mọi chương trình của cùng người dùng đọc bộ nhớ của nhau (mặc định trên Arch). Làm tay: `sudo sysctl kernel.yama.ptrace_scope=0` |
| "Chụp cửa sổ qua KWin" | Bấm "Thêm vào menu ứng dụng" (hoặc chạy `vntext install`), rồi mở lại vntext |
| "Nút chép (wl-copy)" | Cài `wl-clipboard` |
| "Tra từ" | Thêm một từ điển, xem [Tra từ](#tra-từ) |

Trên Linux, nếu file socket `/tmp/.X11-unix/X0` bị mất nhưng XWayland vẫn
chạy, vntext thử socket abstract của đúng display đó, dùng nguyên Xauthority.
Không cần khởi động lại desktop khi đường dự phòng này còn hoạt động. Nếu
không kết nối được X11, cửa sổ chọn game vẫn mở để xem chẩn đoán và thử kết
nối lại định kỳ; việc mở cửa sổ vẫn cần GTK kết nối được Wayland hoặc X11.

vntext có giọng đọc A.I.VOICE2 trong dải nổi, mặc định tắt tự đọc.
Xem [kế hoạch TTS và kết quả kiểm tra](docs/VNTEXT_TTS_PLAN.md).

Tool không lưu gì gắn với một máy: mỗi lần chạy nó đọc lại script từ thư mục game và tìm lại vị trí đọc trong bộ nhớ theo nội dung, không theo địa chỉ. Cấu hình, cache dịch và kho từ điển nằm ngoài repo, ở `~/.config/vntext/`, `~/.cache/vntext/` và `~/.local/share/vntext/`; chép ba thư mục đó sang máy khác là mang theo được mọi thiết lập.

## Sử dụng

### Bắt đầu nhanh

1. Mở game tới chỗ có thoại.
2. Mở vntext từ menu ứng dụng (hoặc gõ `vntext`).
3. Chọn game rồi bấm **Hiện phụ đề**. Quay lại cửa sổ game để thấy dải nổi.

Danh sách ưu tiên tên cửa sổ game; tên tiến trình và engine nằm ở dòng phụ.
Trạng thái **Trích text trực tiếp** nghĩa là đã nhận ra profile.
**Chưa nhận diện · có thể đọc từ ảnh** chưa khẳng định game không được hỗ trợ:
đợi game nạp xong rồi bấm **Nhận diện lại** cạnh danh sách. Tool tự thử lại
sau mỗi 30 giây khi game chưa nhận ra đang được chọn và launcher đang mở.
Đổi game trong cùng emulator cũng có thể bấm nhận diện lại.

Với Higurashi/Umineko trên Ryujinx, tool còn thử tìm trong vùng shared memory
đã cấp phát nếu lần quét trang nhớ thường không thấy kịch bản. Việc này xử lý
trường hợp Linux đã bỏ các bảng trang của alias sau một thời gian; có thể mất
thêm vài giây ở lần nhận diện đó. Không cần chuyển sang OCR chỉ vì gặp trường
hợp này.

**Chi tiết kiểm tra** tự mở khi có vấn đề chặn dải nổi hoặc quyền đọc bộ nhớ.
Những mục bổ sung nằm trong khung thu gọn, không che danh sách game.
**Thiết lập chung** chứa nút **Thêm vào menu ứng dụng**.

Tuỳ chọn tự lưu theo tên ứng dụng: bật dịch, ngôn ngữ đích, nguồn dịch và
mở trang đọc trong trình duyệt. Mặc định dịch sang tiếng Việt bằng Google
nếu chưa có nguồn được cấu hình. Với emulator, hiện các game vẫn dùng chung
khóa cấu hình theo tên tiến trình; cấu hình riêng theo Title ID sẽ làm sau.

Trang web mặc định ở `http://127.0.0.1:8777`; nếu cổng bị chiếm, tool dùng
cổng local trống và mở đúng địa chỉ mới. Dải nổi vẫn hoạt động.

### Icon khay

vntext có icon chữ T ở khay hệ thống. Đóng cửa sổ chọn game chỉ cất nó vào khay.

- **Bấm trái:** hiện cửa sổ chọn game.
- **Chuột phải:** dừng theo dõi game, hoặc thoát hẳn.
- Mở vntext lần nữa sẽ hiện lại cửa sổ đang chạy.

Desktop không có khay thì đóng cửa sổ là thoát.

### Dải nổi

Dải bám theo cửa sổ game và chỉ hiện khi game được chọn. Dải đặt dưới cửa sổ;
nếu thiếu chỗ thì đặt bên trong sát đáy game. Thanh điều khiển và nội dung
nằm trong **một khung nền mờ**, cùng di chuyển và đổi kích thước. Tên người nói, câu Nhật và bản dịch
có phân cấp màu; nguồn text và dịch nằm ở hàng phụ. Đoạn dài có vùng cuộn riêng.
Nhãn **Bản dịch / Nhật / Nhật + dịch** dùng được với mọi ngôn ngữ đích.

Nút **loa nằm ngay bên phải câu Nhật trên dải**, đổi thành nút dừng khi đang
tạo hoặc phát audio. Ở chế độ chỉ hiện bản dịch, loa vẫn ở cạnh câu thoại
và đọc câu Nhật gốc. Chưa chọn giọng thì bấm loa sẽ mở tab Giọng đọc.

Hàng điều khiển trên đầu dải dịu đi khi chuột rời khung, không làm đổi bố cục:

| Nút | Tác dụng |
|---|---|
| Chế độ | Đổi bản dịch, câu Nhật hoặc cả hai; nút ghi rõ chế độ hiện tại. |
| ⋯ | Chỉnh bố cục, chép câu đang hiện, mở OCR, dừng theo dõi game. |
| Bánh răng | Thiết lập Hiển thị, Dịch, Từ điển, OCR và Giọng đọc. |
| Thu gọn (−) | Gấp cả khung thành nút **vntext · Hiện** tại chỗ; bấm nút này để mở lại. |

**Chỉnh bố cục:** mở **⋯ → Chỉnh bố cục…** hoặc nút cùng tên trong tab
Hiển thị. Dải hiện viền và tay nắm ở bốn cạnh/bốn góc. Kéo vùng **⠿ Kéo để
di chuyển** trên tiêu đề để dời dải; kéo cạnh hoặc góc để đổi cả chiều rộng
và chiều cao. Các nút trên tiêu đề vẫn dùng bình thường. Vị trí/kích thước
tự lưu riêng theo game, bám theo cửa sổ game và được giữ trong màn hình.

Trong chế độ chỉnh có thanh **Độ đậm nền** (20–100%) xem thay đổi ngay;
chỉ nền thay đổi, chữ và nút vẫn rõ. **Chiều cao tự động** mặc định bật.
Kéo cạnh trên/dưới hoặc góc sẽ dùng chiều cao cố định; câu dài cuộn bên
trong, thanh điều khiển vẫn hiện. Bật lại ô này để dải tự co giãn theo nội
dung. **Đặt lại** khôi phục vị trí, kích thước và độ đậm nền mặc định, giữ
cỡ chữ/màu/chế độ đang dùng. Bấm **Xong** để khóa thao tác kéo và ẩn công cụ
chỉnh. Thu gọn dải cũng kết thúc chế độ chỉnh.

Quét chữ Nhật và rê chuột tra từ luôn dùng được. Bôi chọn mặc định không
ghi đè clipboard; chuột phải chọn **Chép
phần đã chọn**. Có tùy chọn **Tự chép phần chữ đã chọn**, tự lưu theo ứng dụng.
Ở chế độ chỉ bản dịch và đang khóa vị trí, vùng trống vẫn cho click xuyên
xuống game; các nút trên dải nhận chuột bình thường.

**Lịch sử:** mặc định là nút nhỏ **Lịch sử · N câu**; bấm để mở các nút
trước/sau, hoặc lăn chuột ngay trên hàng này để xem lại.
Cuộn trong vùng câu thoại chỉ cuộn đoạn dài. Khi xem câu cũ, nhãn ghi rõ
**Lịch sử · số câu/tổng số**. Game chuyển câu không kéo người đọc khỏi lịch
sử; bấm **Về câu mới** để tiếp tục theo dõi. Tool giữ tối đa 200 câu; câu
cũ nhất sẽ rời bộ đệm khi đầy. Khi đang ở câu mới nhất, bấm **Thu gọn** trên
hàng lịch sử để trở về nút nhỏ. Có câu mới trong game không tự phát âm thanh.

**Thông báo:** nằm cạnh câu thoại, có nút đóng riêng. Thông báo lưu/đổi nguồn
thành công tự hết sau 5 giây; lỗi giữ lời giải thích và nút khắc phục.
Thông báo không thay mất câu Nhật/bản dịch, và bản dịch về không xóa vùng
bôi chọn nếu câu Nhật chưa đổi. Chuột phải câu Nhật còn có **Dịch lại câu
này** khi có bản dịch từ profile.

**Thiết lập:** tiêu đề và nút đóng hiện ở cả năm tab, nội dung có vùng cuộn
cho màn hình nhỏ. **Esc** luôn đóng bảng. Hiển thị, lựa chọn từ điển và
công tắc OCR áp dụng ngay; API key/model/địa chỉ nguồn dịch chỉ lưu khi bấm
**Lưu cấu hình dịch** hoặc Enter trong ô nhập. Đóng bảng không tự lưu các
ô cấu hình dịch chưa lưu. Việc chọn một nguồn khác sẽ nạp cấu hình đã lưu
của nguồn đó; hoàn tất lưu trước khi đổi nguồn.

**Dừng theo dõi game** đóng phiên trích text/dải nổi và dừng TTS; game vẫn chạy.
Muốn chỉ dừng âm thanh, bấm nút loa đang phát/chờ hoặc Dừng âm thanh trong
tab Giọng đọc.

### Nghe tiếng Nhật bằng A.I.VOICE2

Cần dịch vụ A.I.VOICE2 headless đã cài và đang chạy, có voicebank, cùng
`pw-play` (Arch: `pipewire-audio`, Debian/Ubuntu thường là `pipewire-bin`).
Không cần mở Anki hoặc GUI A.I.VOICE2 để phát từ vntext.

1. Bấm loa trên dải → tab **Giọng đọc** mở ra, tự kiểm tra kết nối/tải giọng.
2. Chọn giọng, bấm **Nghe thử**, chỉnh âm lượng riêng của TTS nếu cần.
3. Đóng thiết lập. Bấm loa để nghe **câu Nhật đang hiện**, dù chế độ hiển thị
   chỉ có bản dịch. Quét một phần câu Nhật → chuột phải → **Nghe phần Nhật
   đã chọn**. Nếu chưa có câu, tool báo rõ thay vì đọc thông báo trên dải.

Tự đọc mặc định **tắt** vì game có thể đã có lồng tiếng. Bật **Tự đọc câu Nhật
mới** trong tab Giọng đọc để đọc các câu tiếp theo. Đổi câu sẽ dừng tiếng cũ,
bỏ câu chờ cũ và chỉ giữ câu mới nhất; bản dịch về không khiến câu Nhật bị
đọc lại. Bấm dừng chỉ dừng lần nghe hiện tại; tắt công tắc tự đọc để dừng cả
các lần tự đọc tiếp theo.

Chuyển ứng dụng hoặc xem lịch sử tạm ngưng tự đọc; quay lại không phát lại
các câu đã bỏ qua. Vẫn có thể bấm nghe thủ công câu lịch sử đang xem.
Giọng, âm lượng, tự đọc được tự lưu. Ryujinx lưu riêng theo Title ID nếu đọc
được từ tiêu đề; không có ID thì UI ghi rõ chỉ lưu theo phiên emulator.
Đổi game trong cùng emulator: dừng theo dõi rồi mở phụ đề lại cho game mới.

**Làm mới giọng** khi thêm/gỡ voicebank. Nếu giọng đã lưu bị gỡ, chọn giọng
khác; không âm thầm thay bằng giọng mặc định. Akane ghi Kansai, các giọng
khác dùng dialect backend khai báo. Endpoint cục bộ mặc định
`http://127.0.0.1:17862`, đổi trong **Kết nối nâng cao** rồi bấm làm mới.
Backend tắt/lỗi không dừng extraction, dịch hoặc tra từ.

**Chuẩn bị audio trước khi nghe** mặc định bật: khi có giọng đã chọn và game
đang được theo dõi, tool chuẩn bị câu ngắn đang hiện ở nền. Sau khi nghe,
nếu profile có kịch bản thì đoán trước một câu kế tiếp để chuẩn bị thêm.
Đây là dự đoán theo thứ tự script, có thể sai khi rẽ nhánh; chỉ phát khi
câu và giọng thực tế khớp. OCR không có dữ liệu câu tiếp theo. Tắt tùy chọn
này để giảm phần tổng hợp dự đoán không dùng tới.

Backend tối ưu sẽ nạp nóng đúng giọng khi chọn/mở phiên và trả PCM dần ngay
khi có chunk đầu; vntext phát qua PipeWire mà không đợi WAV hoàn chỉnh.
Luồng phát được giữ sẵn giữa các lần nghe đã kết thúc. Đổi câu/dừng giữa
chừng đóng đúng player hiện tại; lần sau tạo lại luồng phát. Backend cũ
không hỗ trợ streaming vẫn được dùng qua WAV/memfd như trước.

Cache RAM giữ tối đa 8 câu/32 MiB, mỗi mục dùng lại trong 120 giây, key gồm
endpoint/giọng/text. Không ghi audio tạm ra đĩa từ vntext; backend vẫn lưu
WAV làm cache. Cache backend có thể trả ngay khi engine đang bận. Tác vụ
nghe được ưu tiên hơn tác vụ chuẩn bị chưa chạy, nhưng native synth đang
chạy chưa hủy được: chuyển câu nhanh vẫn có thể phải chờ phần việc cũ kết
thúc. Lần nạp voicebank bị swap hoặc đổi giọng vẫn có thể mất vài giây.
Chưa có chỉnh tốc độ hoặc tự phát hiện câu đã lồng tiếng.

Chi tiết cài backend thử, giao thức streaming và rollback:
[integrations/aivoice2/README.md](integrations/aivoice2/README.md).

### Tra từ

Khi dải hiện chữ Nhật, rê chuột lên một từ là nghĩa hiện ra trong khung bên cạnh, trình bày như Yomitan. Động từ và tính từ đã chia (食べさせられた, 高くなかった) được đưa về dạng gốc bằng chính bảng luật của Yomitan; tên viết romaji (Shinjuku) cũng tra được.

Tool không tự tải từ điển nào. Thêm từ điển như sau:

1. Bánh răng → tab **Từ điển** → **Tải từ điển**. Trình duyệt mở <https://yomitan.wiki/dictionaries/>.
2. Tải file zip của từ điển bạn muốn (nên có một từ điển nghĩa và một từ điển tần suất). Không cần giải nén.
3. Bấm **Thêm từ điển…** rồi chọn các file, hoặc kéo thả file zip vào tab.

Mỗi từ điển có công tắc bật/tắt, nút đổi thứ tự (từ điển đứng trước hiện nghĩa trước) và nút xoá. Hỗ trợ từ điển Yomitan định dạng bản 3: nghĩa, tần suất và nhãn; ảnh, cao độ và IPA bị bỏ qua.

Ô **Tra từ bằng** chọn nguồn: "Từ điển trong tool", "Yomitan (Firefox)" hoặc "Tự chọn" (mặc định: có từ điển đã nhập thì dùng nó, chưa có thì hỏi Yomitan của trình duyệt). Dùng Yomitan của trình duyệt cần [yomitan-api](https://github.com/yomidevs/yomitan-api) đã cài, trong Yomitan Settings bật Advanced rồi bật General → Enable Yomitan API, và trình duyệt đang mở.

### Dịch

| Nguồn | `--via` | Key | Model mặc định |
|---|---|---|---|
| Google Dịch | `google` | không cần | |
| Gemini | `gemini` | `GEMINI_API_KEY` | Flash-Lite mới nhất mà API liệt kê |
| OpenAI | `openai` | `OPENAI_API_KEY` | phải ghi |
| DeepSeek | `deepseek` | `DEEPSEEK_API_KEY` | `deepseek-chat` |
| Claude | `claude` | `ANTHROPIC_API_KEY` | `claude-opus-5-5` |
| Server nói kiểu OpenAI (Ollama, LM Studio…) | `local` | không bắt buộc | phải ghi |

- **Nhập key và model:** bánh răng → tab Dịch. Nút "Danh sách" tải danh sách model của nguồn. Tool đang chạy đổi nguồn ngay khi lưu.
- **Các nguồn dùng model** dịch nguyên đoạn kèm 8 đoạn trước làm ngữ cảnh, và mỗi lần gọi dịch luôn 12 đoạn kế tiếp. Bản dịch lưu ở `~/.cache/vntext/<ngôn ngữ>.tsv`, đọc lại không tốn lần gọi nào.
- **Dịch lại:** chuột phải lên câu trên dải → "Dịch lại câu này". Bản dịch mới thay bản cũ, cả trong cache.
- **Đoạn bị bộ lọc chặn:** model từ chối một đoạn (bộ lọc nội dung của Gemini, Claude từ chối) thì tool chia nhỏ lần gọi để tìm đúng đoạn đó, các đoạn khác vẫn được model dịch, còn đoạn đó do Google dịch. Bộ lọc của Gemini đã được hạ hết mức API cho phép.
- **Request treo** quá 90 giây thì bị bỏ và gọi lại, không làm dịch ngừng hẳn.
- **Google Dịch** đi qua cổng không chính thức, dịch từng câu rời không có ngữ cảnh, và không lưu cache. Nó tự thay vào khi nguồn đã chọn thiếu key, và cho riêng câu đang đọc khi nguồn đã chọn lỗi.
- Mới chỉ Gemini và Google được chạy thử thật; các nguồn còn lại viết theo tài liệu API.

### OCR cho game chưa hỗ trợ

Game chưa có profile engine vẫn đọc được: tool chụp khung thoại mỗi khi nó đổi và đứng yên, gửi ảnh (PNG xám, vài KB) cho Gemini, một lần gọi trả về người nói, câu tiếng Nhật và bản dịch.

- **Cần key Gemini** (bánh răng → tab Dịch), kể cả khi nguồn dịch đã chọn là nguồn khác.
- **Lần đầu với mỗi game** tool hỏi khung thoại ở đâu: màn hình tối lại, kéo chuột quanh khung thoại. Khoanh lại bằng bánh răng → tab OCR → "Khoanh lại khung thoại".
- OCR có thể sai chữ, không dịch trước được, và mỗi câu tốn một lần gọi.
- Model ghi trong thiết lập không còn tồn tại thì tool tự chuyển sang Flash-Lite mới nhất.

### Game dùng Wayland driver của Wine

Mặc định tool hỏi máy chủ X về cửa sổ game. Trên KDE, nếu sau khoảng hai giây vẫn không có cửa sổ X11 nào của game, nó chuyển sang hỏi KWin:

- **Vị trí và focus:** tool nạp một KWin script báo vị trí cửa sổ về qua D-Bus, và gỡ script khi thoát.
- **Chụp khung thoại:** qua `org.kde.KWin.ScreenShot2`. KWin chỉ cho chương trình đã được khai trong menu ứng dụng, nên phải chạy `vntext install` một lần.
- `VNTEXT_SCREEN=kwin` ép hỏi KWin cho mọi cửa sổ, `VNTEXT_SCREEN=x11` chỉ hỏi X.

Ngoài KDE chưa hỗ trợ cửa sổ Wayland: hãy tắt Wayland driver của Wine để game chạy qua XWayland.

### Dòng lệnh

Cửa sổ chọn game chỉ là lớp vỏ: nó chạy lại chính `vntext` với các tham số dưới đây.

```
vntext auto <pid|tên> --overlay --translate          # game có profile
vntext ocr  <pid|tên> --overlay --translate          # game chưa có profile
vntext auto ryujinx --web --translate                # không dải nổi, đọc trên trang web
```

`<tên>` là tên process, khớp một phần và không phân biệt hoa thường. Linux chỉ giữ 15 byte đầu của tên process, nên chỉ cần gõ phần đầu tên file chạy.

| Lệnh | Tác dụng |
|---|---|
| (không tham số) | Mở cửa sổ chọn game |
| `install` | Thêm vntext và icon vào menu ứng dụng, đặt lệnh `vntext` ở `~/.local/bin` |
| `doctor` | Kiểm tra máy có đủ thứ tool cần không, nói thiếu gì và sửa thế nào |
| `auto <pid\|tên>` | Đọc game có profile; tự nhận engine |
| `ocr <pid\|tên>` | Đọc khung thoại từ ảnh chụp qua Gemini |
| `shot <pid\|tên> <file.png>` | Ghi ra đúng tấm ảnh khung thoại mà OCR sẽ gửi đi, để soi khi OCR đọc sai |
| `strings <pid\|tên>` | In mọi chuỗi tiếng Nhật đang có trong bộ nhớ |
| `find <pid\|tên> <chuỗi>` | Tìm địa chỉ một chuỗi đã biết, ở cả ba bảng mã |
| `watch <pid\|tên> <địa chỉ hex> <utf8\|utf16\|sjis>` | Theo dõi một địa chỉ cố định |

| Cờ | Tác dụng |
|---|---|
| `--overlay` | Hiện dải nổi cạnh cửa sổ game |
| `--translate`, `--to LANG` | Dịch, và ngôn ngữ đích (mặc định `vi`) |
| `--via TÊN`, `--model ID`, `--google` | Nguồn dịch và model; `--google` là `--via google` |
| `--web`, `--port N`, `--open` | Phục vụ trang đọc ở `127.0.0.1:8777` hoặc cổng đã chọn; cổng bị chiếm thì tự chọn cổng trống và in địa chỉ thực tế. `--open` mở đúng địa chỉ đó trong trình duyệt |
| `--clip` | Đưa mỗi đoạn vào clipboard (`wl-copy`) |
| `--pick` | `ocr`: khoanh lại khung thoại |
| `--raw` | In text nguyên dạng engine lưu, gồm cả mã điều khiển |
| `--debug` | In mọi lần ô vị trí đổi giá trị; với `ocr`, lưu ảnh vừa gửi vào `~/.cache/vntext/ocr.png` |
| `--min N`, `--all` | `strings`/`find`: độ dài chuỗi tối thiểu, và quét cả vùng chỉ đọc |

### File tool ghi

| File | Nội dung |
|---|---|
| `~/.config/vntext/games.json` | Tuỳ chọn của từng game trong cửa sổ chọn game |
| `~/.config/vntext/overlay.json` | Vị trí, bề rộng, chiều cao tự động/cố định, độ đậm nền, cỡ chữ, màu, chế độ của dải theo từng game |
| `~/.config/vntext/translate.json` | Nguồn dịch, key, model (quyền 0600) |
| `~/.config/vntext/regions.tsv` | Khung thoại đã khoanh cho OCR, theo từng game |
| `~/.config/vntext/lookup.json` | Nguồn tra từ đã chọn |
| `~/.local/share/vntext/dict.sqlite` | Các từ điển đã nhập |
| `~/.cache/vntext/<ngôn ngữ>.tsv` | Bản dịch đã có |

## Game đã hỗ trợ

| Game | Engine | Nền tảng | Kết quả |
|---|---|---|---|
| Umineko no Naku Koro ni Saku | Entergram shin | Switch, Ryujinx 1.3.3 | Chính xác từng đoạn, đã kiểm tra |
| Higurashi no Naku Koro ni Hou | Entergram shin | Switch, Ryujinx 1.3.3 | Chính xác từng đoạn, đã kiểm tra |
| 牝教師 1, 2 | BISHOP (BSXScript 3.0) | Windows, Proton-GE qua Faugus | Chính xác từng câu, đã kiểm tra |
| 牝教師 4, Sansha Mendan (bản tiếng Anh) | BISHOP (BSXScript 3.1) | Windows, Proton-GE qua Faugus | Chính xác từng câu, đã kiểm tra |
| 家属～母と姉妹の嬌声, Shihai no Kyoudan (bản tiếng Anh) | BISHOP (BSXScript 3.2) | Windows, Proton-GE qua Faugus | Chính xác từng câu, đã kiểm tra |
| 牝教師 3, 淫獄の放課後 1–2, 隷従の制服, Shihai no Kyoudan 2 (bản tiếng Anh) | BISHOP (BSXScript 3.3) | Windows, Proton-GE qua Faugus | Chính xác từng câu, đã kiểm tra |
| 催眠術2, 催眠術2 URA | BLACKRAINBOW spt | Windows, Proton-GE qua Faugus | Chính xác từng câu, đã kiểm tra |
| 催眠術3, 催眠術4, 催眠術 RE, WIZARDLINKS | MED | Windows, Proton-GE qua Faugus | Chính xác từng khung thoại, đã kiểm tra |
| 極限痴漢特異点 1, 2, 3, NEO và hai bản ハーレム | ArkEngine (Unity, Mono) | Windows, Proton-GE qua Faugus | Chính xác từng khung thoại, đã kiểm tra; khung lựa chọn đã kiểm tra trên bản 3 |
| 聖奴隷学園 1, 2, 3, Saimin Seishidou Secret Lesson (bản tiếng Anh) | YU-RIS (479, 488, 494) | Windows, Proton-GE qua Faugus | Chính xác từng câu, đã kiểm tra |
| VenusBlood Savior, VenusBlood Valkyrie | Ninetail (Unity, Mono 32-bit) | Windows, Proton-GE qua Faugus | Chính xác từng khung thoại, đã kiểm tra (Savior 2.04, Valkyrie 1.04) |
| 封緘のグラセスタ | AGE của Eushully (SYS4) | Windows, Proton-GE qua Faugus | Chính xác từng khung thoại, kể cả khung lựa chọn, đã kiểm tra |
| 信天翁航海録 | Liar-soft (script `.gsc`) | Windows, Proton-GE qua Faugus | Chính xác từng câu, kể cả khung lựa chọn, đã kiểm tra |
| 廃村少女 弐 | Escu:de | Windows, Proton-GE qua Faugus | Chính xác từng trang thoại, đã kiểm tra (kể cả ngay sau khi load save) |
| アマカノ ～Perfect Edition～ | CatSystem2 | Windows, Proton-GE qua Faugus | Chính xác từng lần click, có tên nhân vật chính, đã kiểm tra; chưa làm khung lựa chọn (game chọn trên bản đồ) |
| Game engine khác | chưa có profile | cửa sổ X11 | Đọc khung thoại bằng ảnh chụp (lệnh `ocr` hoặc nút OCR trên dải nổi); đã kiểm tra |

Nhiều hãng dùng chung engine, nên game không có trong bảng vẫn có thể chạy được: cứ mở vntext xem nó có nhận ra không.

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
  engines.md         Ghi chú kỹ thuật từng engine
  PLAN.md            Kế hoạch đã dùng để làm phần giao diện cho người mới
```

Tool gồm ba tầng, chỉ tầng giữa phụ thuộc engine:

1. **Đọc bộ nhớ** (`mem.rs`). Liệt kê vùng nhớ từ `/proc/<pid>/maps`, đọc qua `/proc/<pid>/mem`, chỉ đọc trang đang có trong RAM và khử các vùng trùng nhau. Tốc độ quét khoảng 1 GB/giây.
2. **Tìm ra câu đang hiển thị** (`engines/`, hoặc `ocr.rs` cho game chưa có profile). Một profile đọc toàn bộ script của game thành danh sách đoạn thoại, rồi tìm trong bộ nhớ ô mà engine dùng để giữ vị trí đang đọc. Vòng đọc trong `main.rs` đọc ô đó mỗi 50 ms; ô trỏ sang đoạn khác thì đoạn mới được phát đi.
3. **Đưa text ra.** Mỗi đoạn được đẩy vào `Hub` (`web.rs`) dưới dạng sự kiện (`message`, `translation`, `notice`). Dải nổi, trang web và luồng dịch đều nghe từ đó, nên thêm một engine không phải sửa phần hiển thị.

Cách từng engine cất script và giữ vị trí đọc được ghi ở [docs/engines.md](docs/engines.md).

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
4. Thêm game vào bảng [Game đã hỗ trợ](#game-đã-hỗ-trợ), và ghi cách engine cất script và giữ vị trí vào [docs/engines.md](docs/engines.md): đó là thứ người sau cần nhất khi một bản game mới làm profile hỏng.

Trong lúc chưa có profile, game vẫn chơi được bằng OCR, hoặc bằng Textractor chạy trong cùng prefix Wine.

## Giới hạn

- **Game chưa có profile chỉ đọc được bằng ảnh.** OCR có thể sai chữ, không dịch trước, mỗi câu tốn một lần gọi Gemini, và hiện chỉ dùng được với Gemini.
- **Text động thì không thấy.** Thứ game ghép lúc chạy (tên do người chơi đặt, số liệu trong câu, giao diện chiến đấu) không nằm trong script.
- **Chưa bám theo từng lần bấm.** Một đoạn thoại gồm vài câu mà game hiện dần; tool đưa ra cả đoạn ngay khi game tới đó.
- **Tua nhanh có thể sót đoạn**, vì ô vị trí chỉ được đọc mỗi 50 ms.
- **Khung lựa chọn** đã có ở mọi profile trừ shin (Umineko, Higurashi).
- **Dải nổi cần layer-shell**: không chạy trên GNOME. Cửa sổ Wayland của game chỉ được hỗ trợ trên KDE.
- **Bản game mới đổi cấu trúc engine** có thể làm một profile không tìm ra vị trí đọc. Khi đó tool báo không tìm thấy, không hiện sai text.

## Port sang Windows (tham khảo)

> **Chưa được thực hiện.** Mục này là bản phác thảo lý tưởng để ai muốn port có điểm xuất phát. Nó dựa trên việc đọc code, chưa build hay thử trên Windows. Hiện tool vẫn chỉ chạy trên Linux.

Port khả thi. Lõi của tool (các engine) gần như không phụ thuộc hệ điều hành; phần phải làm lại là lớp đọc bộ nhớ và giao diện.

### Phần dùng lại được

- Mọi `engines/*/text.rs` chỉ xử lý byte (archive, giải mã, parse bytecode), chạy nguyên trên Windows.
- Các `engines/*/mod.rs` chỉ chạm hệ điều hành qua bốn hàm của `mem::Proc`: `read`, `scan`/`scan_where`, `game_dirs`, `game_file(s)`. Cài lại `Proc` là đủ cho cả 11 engine.
- Dùng lại nguyên: `dict/`, `yomitan.rs`, `translate.rs`, `web.rs`, `text.rs`, và các crate `rusqlite`, `ureq`, `serde_json`, `encoding_rs`.
- Hai chỗ dùng `std::os::unix::fs::FileExt` (`engines/age/text.rs`, `engines/yuris/mod.rs`) đổi sang `std::os::windows::fs::FileExt::seek_read`.

### Phần phải viết lại

| Phần | Linux hiện tại | Windows |
|---|---|---|
| `mem.rs` | `/proc/<pid>/{maps,mem,pagemap}` | `OpenProcess`, `VirtualQueryEx` thay cho `maps`, `ReadProcessMemory` thay cho `mem` |
| Quyền đọc (`doctor.rs`) | `kernel.yama.ptrace_scope` | Không cần; chỉ cần tool và game cùng mức quyền (game chạy admin thì tool cũng phải admin) |
| Cửa sổ và chụp ảnh (`screen.rs`, `kwin.rs`) | x11rb, KWin | `EnumWindows`, `GetWindowRect`, `GetForegroundWindow`, chụp bằng `BitBlt` hoặc Windows.Graphics.Capture |
| Dải nổi (`overlay.rs`) | GTK4 + layer-shell | Cửa sổ luôn nổi, trong suốt, bấm xuyên qua được (`WS_EX_TOPMOST`, `WS_EX_LAYERED`, `WS_EX_TRANSPARENT`), bám theo cửa sổ game |
| Khay, cài đặt, launcher | `ksni`/`zbus`, symlink, `pkexec`, `xdg-open`, `wl-copy`, `libc::kill`, `prctl` | crate `tray-icon`, shortcut `.lnk`, `ShellExecute`, API clipboard, `TerminateProcess` và job object |

Có thể bỏ hẳn nhánh Wayland/KWin/XWayland. Trên Windows game chạy native nên cũng không còn chuyện tìm tiến trình qua Wine hay Proton.

### Cách làm đề xuất

1. **Tách `Proc` theo nền tảng** bằng `#[cfg(unix)]` / `#[cfg(windows)]` (hoặc một trait), giữ nguyên chữ ký `read`, `scan`, `game_dirs`, `game_file`. Thêm `mem_windows.rs`; trong `Cargo.toml` chuyển `x11rb`, `libc`, `ksni`, `zbus`, `gtk4-layer-shell` vào `[target.'cfg(unix)'.dependencies]`.
2. **Chạy bằng dòng lệnh trước**: in text của một engine đơn giản (BISHOP hoặc SPT) ra terminal. Mốc này cho thấy lõi chạy được khi chưa cần giao diện.
3. **Dùng trang web làm giao diện tạm** (`web.rs`), để có thứ dùng được trước khi có dải nổi.
4. **Làm dải nổi, khay và launcher cuối cùng.** Đây là phần tốn công nhất, riêng `overlay.rs` đã hơn 2.000 dòng. GTK4 chạy được trên Windows nhưng không có layer-shell, nên cần đặt cờ cửa sổ bằng Win32 hoặc đổi toolkit.

### Điều cần kiểm tra khi làm thật

- **`Proc::game_dirs`** hiện suy ra thư mục game từ `cwd` và cmdline của umu-run/proton. Trên Windows lấy từ `QueryFullProcessImageName`.
- **Bộ lọc vùng nhớ** (`Mapping::wanted`) lọc theo quyền `rw` và loại vùng thiết bị. Cần ánh xạ sang `MEMORY_BASIC_INFORMATION` (`State`, `Protect`, `Type`).
- **Quét `thorough`** dùng `pagemap` để bỏ qua trang chưa nằm trong RAM. Windows không có thứ rẻ tương đương (`QueryWorkingSetEx` chậm hơn), nên quét kỹ có thể chậm hơn.
- **Offset bộ nhớ của từng engine** (YU-RIS, AGE, Ninetail…) là của chính game nên về nguyên tắc không đổi, nhưng README mới chỉ kiểm tra trên Proton. Cần thử lại từng engine trên bản Windows native.
- **Shin trên Ryujinx** (Umineko, Higurashi) dựa vào cách Ryujinx cấp phát bộ nhớ guest trên Linux (`LIVE_CELL` trong `engines/shin/mod.rs`). Bản Windows của Ryujinx cấp phát khác, nên đây có thể là engine khó port nhất.
- **Đường dẫn và tên file**: dấu phân cách `\`, không phân biệt hoa thường, tên Shift-JIS. Rà các chỗ so khớp tên file như `p.game_file(|name| name == ...)`.
- **Tiến trình game chạy quyền admin** sẽ không đọc được nếu tool chạy quyền thường.

Nếu chỉ cần đọc text game Windows mà không cần dịch trước hay tra từ trên dải nổi, [Textractor](https://github.com/Artikash/Textractor) vẫn là lựa chọn hợp lý hơn (xem mục bên dưới).

## So với Textractor và agent

[Textractor](https://github.com/Artikash/Textractor) và [agent](https://github.com/0xDC00/agent) lấy text bằng cách hook vào game lúc nó chạy. vntext đi đường khác: đọc script từ file của game, rồi chỉ nhìn bộ nhớ để biết game đang ở câu nào.

| | Textractor | agent (Frida) | vntext |
|---|---|---|---|
| Cách lấy text | Hook hàm trong process game | Hook hàm trong process game hoặc emulator | Đọc script từ file game, đọc vị trí từ bộ nhớ |
| Can thiệp vào game | Inject DLL | Inject Frida | Không; chỉ đọc từ bên ngoài |
| Thứ nó phụ thuộc | Engine và cách game gọi hàm vẽ chữ | Phiên bản game và phiên bản emulator | Định dạng script và cấu trúc dữ liệu của engine |
| Game được hỗ trợ | Rất nhiều; game lạ vẫn thử được bằng hook chung | Theo danh sách script có sẵn | Chỉ các engine có profile (xem [Game đã hỗ trợ](#game-đã-hỗ-trợ)) |
| Game chưa hỗ trợ | Tự tìm hook hoặc H-code | Viết script hook mới | Viết profile mới bằng Rust |
| Thứ nhận được | Chuỗi tại thời điểm hàm được gọi | Chuỗi tại thời điểm hàm được gọi | Cả đoạn thoại kèm tên người nói, và toàn bộ script |
| Nền tảng | Windows; trên Linux phải chạy trong cùng Wine prefix với game | Windows, Linux, macOS | Linux |

Dự án bắt đầu vì agent dừng hoạt động trên Ryujinx 1.3.3 sau khi emulator chuyển sang .NET 9, và vì đưa Textractor vào từng Wine prefix của từng game trên Linux rất phiền. Cái giá của cách làm này là ít game hơn nhiều, và mỗi engine là một lần dịch ngược. Nếu chơi trên Windows, Textractor vẫn là lựa chọn hợp lý hơn.

## Nguồn tham khảo

- [DCNick3/shin](https://github.com/DCNick3/shin): bản viết lại engine Entergram; nguồn của định dạng SNR, bảng ký tự và cú pháp lệnh.
- [crskycode/BSXScript_Tool](https://github.com/crskycode/BSXScript_Tool): công cụ xuất/nhập text của `bsxx.dat`; nguồn của định dạng BSXScript.
- [GARbro, ArcMED.cs](https://github.com/morkt/GARbro/blob/master/ArcFormats/DxLib/ArcMED.cs): định dạng gói `.med`.
- Bộ công cụ dịch cho engine MED 催眠術3-4-re (`MED.py`, `SCE.py`, https://github.com/aquaplatinumberlitz/saiminjutsu-fudegaki-backrainbow-tools): cách dò khoá và định dạng script.
- [shimamura-sakura/yuri](https://github.com/shimamura-sakura/yuri): bộ dịch ngược và tài liệu định dạng của YU-RIS.
- Bộ công cụ patch tiếng Anh cho 催眠術2 và 催眠術2 URA (https://github.com/aquaplatinumberlitz/saiminjutsu-fudegaki-backrainbow-tools, không rõ tác giả): nguồn của định dạng gói `*_spt.dat` và file `.spt`.
- [0xDC00/agent](https://github.com/0xDC00/agent): text hooker dùng Frida, điểm xuất phát của dự án.
- [Artikash/Textractor](https://github.com/Artikash/Textractor): text hooker phổ biến nhất trên Windows; so sánh ở mục [So với Textractor và agent](#so-với-textractor-và-agent).
- [Kelebek1/Eushully-Decompiler](https://github.com/Kelebek1/Eushully-Decompiler): định dạng script và gói của engine AGE.
- [Love-in-idleness/LiarsoftTool](https://github.com/Love-in-idleness/LiarsoftTool): định dạng gói XFL và script GSC của Liar-soft.
- [Chenx221/EscudeTools](https://github.com/Chenx221/EscudeTools): định dạng gói, database và script của Escu:de.
- [GARbro, ArcINT.cs](https://github.com/morkt/GARbro/blob/master/ArcFormats/CatSystem/ArcINT.cs): gói `.int` (KIF) của CatSystem2 và cách lấy khoá từ file exe.
- [trigger-segfault/TriggersTools.CatSystem2](https://github.com/trigger-segfault/TriggersTools.CatSystem2): định dạng script `.cst` (CatScene) của CatSystem2.
- [yomidevs/yomitan](https://github.com/yomidevs/yomitan): bảng luật chia từ và các bộ tiền xử lý chữ tiếng Nhật mà `src/dict/` dùng lại; cách trình bày popup tra từ.
