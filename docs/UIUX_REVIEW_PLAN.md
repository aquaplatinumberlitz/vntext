# Cải tiến UI/UX trước TTS

> Cập nhật kiến trúc 2026-11-07: Hồ sơ triển khai UI đã merge. Nhánh/worktree và các yêu cầu chờ test dưới đây theo thời điểm lịch sử; hướng dẫn hiện hành ở README.md của sản phẩm và docs/README.md.

Ngày 2026-11-06: chủ dự án đã thử UI/TTS và duyệt merge vào main. Các ghi
chú về nhánh thử/chờ nghiệm thu dưới đây giữ lại làm lịch sử triển khai.

Sau khi chủ dự án xác nhận dải/nhận diện đã ổn, TTS được duyệt triển khai
tiếp ngay trên nhánh này. Phần dưới ghi lịch sử đợt UI; chức năng và kết quả
TTS hiện tại xem [VNTEXT_TTS_PLAN.md](VNTEXT_TTS_PLAN.md).

Được duyệt ngày 2026-11-06. Nhánh `feat/uiux-before-tts`, worktree riêng
`/home/admin/Projects/vntext-uiux`. Chưa commit hoặc merge cho đến khi chủ
dự án nghiệm thu và yêu cầu. Các sửa lỗi khởi động/nhận diện Higurashi và
cổng web từ worktree `vntext` được giữ làm nền; bản gốc không bị thay đổi.
Bản diff nền nằm trong `target/uiux-baseline/inherited.patch` (không vào Git).

## Phạm vi đã duyệt

1. Thiết lập: tiêu đề/nút đóng chung, cuộn được trên màn hình nhỏ; phân biệt
   tự lưu với nút lưu cấu hình dịch. Giữ GTK và các chức năng hiện có.
2. Dải nổi: thông báo nằm bên câu thoại; thông báo thành công tự hết hạn,
   lỗi có thể đóng. Giữ lựa chọn chữ khi chỉ có bản dịch/trạng thái cập nhật.
3. Lịch sử: không tự nhảy khỏi câu cũ, có trước/sau/về câu mới nhất, xử lý
   giới hạn 200 câu. Cuộn vẫn có thể xem lịch sử ngoài vùng nút/chữ được chọn.
4. Khóa vị trí độc lập với chọn chữ; tùy chọn tự chép phần chọn (mặc định tắt).
5. Thanh công cụ ngắn: ẩn/hiện, chế độ, menu thêm, thiết lập. Chỉnh vị trí,
   chép, OCR và dừng theo dõi trong menu thêm. TTS sẽ bổ sung sau nghiệm thu.
6. Launcher: trạng thái dễ đọc, nút nhận diện lại cạnh danh sách, chi tiết
   kiểm tra thu gọn khi không chặn chạy, thao tác thêm menu ở thiết lập chung.
7. Đồng nhất khoảng cách, vùng bấm, màu nhấn; nhãn rõ nghĩa để sau này phân
   biệt trích text với phát âm thanh. README cập nhật cùng hành vi.

## Giới hạn

Đợt UI ban đầu không đổi engine/profile. Sau khi chủ dự án báo mất nhận diện
Higurashi, bổ sung sửa quét shared memory như ghi bên dưới. Không gọi API trả phí, không mở/điều khiển game,
không thay Faugus, không triển khai TTS hoặc đổi schema backend A.I.VOICE2.
Thiết lập cũ phải đọc được. Lựa chọn giọng theo Title ID để giai đoạn TTS.

## Kiểm chứng và nghiệm thu

- Build release, test unit cho lịch sử/giới hạn/câu mới, thông báo/selection
  và tương thích cấu hình cũ; kiểm tra diff.
- Kiểm tra giao diện tổng hợp trong phiên đồ họa cô lập khi khả thi; không
  giả lập đầu vào trên desktop đang dùng.
- Mở bản nhánh riêng cho chủ dự án thử launcher, thiết lập bốn tab, chọn
  chữ/chép, lịch sử, ẩn/hiện và đóng phiên đọc trong game thật.
- Chỉ merge khi chủ dự án đồng ý; chỉ commit khi được yêu cầu.

## Kết quả kiểm tra

- `cargo test --release --locked`: 122 đạt, 0 lỗi, 3 bỏ qua. Ba bài bỏ
  qua gồm Gemini thật và hai bài GUI chỉ chạy trong compositor riêng.
- `tools/uiux-smoke.py target/release/deps/vntext-8fa78a1d2254f22d`:
  hai bài GUI đều đạt ở 1024×768 và 800×600. Chạy KWin virtual/D-Bus/cấu
  hình riêng; không mở game hay gửi đầu vào tới desktop đang dùng.
- GUI kiểm tra thực: bốn tab luôn có nút đóng; giới hạn kích thước bảng;
  chọn chữ không mất khi dịch/thông báo về; khóa không chặn chọn chữ;
  trước/sau/về câu mới, thông báo, ẩn/hiện, menu ⋯ và chuột phải.
  Launcher không cho bắt đầu khi chưa chọn game hợp lệ. Ảnh render ở
  `target/diagnostics/uiux/{1024x768,800x600}/`.
- `cargo build --release --locked` và `git diff --check` đạt.
- Đối chiếu trên desktop KDE thật: nền dòng chưa chọn bị theme hệ thống
  đặt trắng; đã khai báo nền row rõ ràng, build/test GUI lại đều đạt.
- Launcher mới có mục mở riêng ở
  `~/.local/share/applications/vntext-uiux-preview.desktop`.
  `vntext.desktop` và binary/worktree bản cũ giữ nguyên. Mỗi phiên desktop
  chỉ mở một launcher; muốn trở lại bản cũ, thoát vntext từ khay trước.

## Cần chủ dự án nghiệm thu

1. Chọn Ryujinx đang chạy → Hiện phụ đề → quay lại game.
2. Chọn chữ khi vị trí đang khóa; chuột phải chép; rê chuột tra từ.
3. Xem lịch sử rồi chuyển câu game: lịch sử đứng yên; Về câu mới tiếp tục bám.
4. Mở bốn tab, đóng bằng X/Esc; kéo vị trí qua menu ⋯; ẩn/hiện;
   dừng theo dõi quay về launcher, game vẫn chạy.
5. Kiểm tra hiển thị thực tế khi kéo/đổi cỡ cửa sổ và nhiều màn hình.
   Bài GUI cô lập chưa nghiệm thu focus/popover/clipboard trong game thật,
   chưa gọi OCR bằng key, nhập từ điển thật hoặc thay nguồn dịch trả phí.

Tại lần kiểm tra cuối, `auto 3184309` không tìm ra profile trên cả binary
bản cũ lẫn bản UI mới (stdout bỏ đi, không dịch/OCR/web). Không sửa engine
trong đợt UI đầu tiên; nguyên nhân đã được xác định và sửa ở mục tiếp theo.

## Sửa mất nhận diện Higurashi sau thời gian chạy

Chủ dự án báo lại lỗi nhận diện sau khi mở bản UI mới. Cửa sổ Ryujinx vẫn
có trong X11, PID không đổi. Đối chiếu có giới hạn cho thấy:

- Mapping bao trọn RAM guest dài 4 GiB nhưng pagemap của alias này chỉ
  còn 125 trang present, không có trang swapped. Quét theo pagemap chỉ đọc
  512.000 byte và không thấy `SNR `. Các alias chỉ đọc cũng không thấy.
- Backing file `/dev/shm/Ryujinx-* (deleted)` vẫn có 3.523.780.608 byte đã
  cấp phát. Quét extent tìm thấy header SNR, kích thước 26.104.192 byte.
  Không in hay lưu hàng loạt câu thoại của game.
- Kết luận: thiếu PTE ở alias không đồng nghĩa backing file không có dữ
  liệu. Không cần đổi GUI hay chuyển sang OCR để giải quyết lỗi này.

Sửa `mem.rs::scan_shared`: đối chiếu device/inode của fd với mapping shared
ghi được bao trọn file; quét theo SEEK_DATA/SEEK_HOLE, bỏ hole và thiết bị,
khử trùng file. Chỉ mở fd có liên kết `/dev/shm/`, chế độ nonblocking để
tránh bị kẹt nếu emulator đóng/tái sử dụng fd. Không đổi offset fd của game.
`shin::find` chỉ dùng đường này sau khi quét thường không nhận được kịch bản;
giữ nguyên parser, tiêu chuẩn nhận diện và cách bám vị trí thoại.

Kiểm chứng:

- Test tổng hợp: một mapping shared chưa có PTE khiến quét cũ đọc 0 byte;
  đường mới tìm đúng marker trong 2 trang đã cấp phát. Không đọc hole,
  số block file không tăng, offset fd gốc không đổi; filter vẫn được tôn trọng.
- Toàn bộ unit test: 123 đạt, 0 lỗi, 3 bỏ qua (GUI riêng và Gemini thật).
- Chạy `auto` không dịch/OCR/web, bỏ stdout: log xác nhận recovered shared
  scenario, `shin (Higurashi Hou layout), 190588 passages`, bám vị trí đang đọc.
  Chủ dự án còn nghiệm thu tương tác chuyển câu/dải nổi trong game thật.
- Log và các probe chỉ ghi metadata ở `target/diagnostics/recognition/`.
- Đã mở lại launcher bằng release mới; log của chính launcher xác nhận
  `engine for process 3184309: shin (Higurashi Hou layout)`.
