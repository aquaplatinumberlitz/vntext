# Dải nổi hợp nhất

Nhánh triển khai: `feat/unified-overlay`, dựa trên `main` tại `02de868`.
Ngày 2026-11-06, chủ dự án đã thử bản mới, xác nhận "ok rồi" và duyệt merge/push.

Ngày 2026-11-07, chủ dự án duyệt commit/merge/push đợt tiếp theo trên
`fix/ui-contrast-spacing`: tương phản bộ chỉnh số, padding launcher, vùng
thoại tự giãn, kéo dải ổn định và kéo icon thu nhỏ. Đợt này kèm sửa mở lại
shin và quota WAV backend; xem `engines.md` và `VNTEXT_TTS_PLAN.md`.

## Bố cục

- Một khung nền mờ chứa hàng điều khiển, câu Nhật với nút loa, bản dịch và lịch sử.
- Hàng điều khiển dịu đi khi chuột rời khung; chiều cao không đổi.
- Thu gọn thành **icon vntext**, cùng artwork desktop/tray. Bấm để mở, giữ và kéo để dời icon; không phụ thuộc icon theme đã cài. Vị trí icon lưu riêng; mở lại dải gần icon và giữ kích thước cũ.
- Lịch sử mặc định chỉ có một nút nhỏ. Mở ra mới hiện trước/sau, vị trí và nút về câu mới.
- Mặc định nằm dưới game, thiếu chỗ thì nằm trong sát đáy game. Giới hạn vị trí theo màn hình chứa tâm game.
- **⋯ → Chỉnh bố cục…** hiện viền, tám tay nắm resize và vùng kéo trên tiêu đề. Bấm **Xong** để khóa lại; chữ Nhật vẫn chọn/tra được.
- Chỉnh **Độ đậm nền** 20–100%, xem trực tiếp kèm phần trăm. Chỉ nền đổi độ đậm.
- Resize ngang giữ chế độ cao hiện tại; resize dọc/góc chuyển sang cao cố định. Bật **Chiều cao tự động** để theo nội dung. Câu dài cuộn trong khung; tiêu đề luôn ngoài vùng cuộn.
- **Đặt lại** khôi phục vị trí/kích thước/độ đậm nền; giữ cỡ chữ, màu và chế độ hiển thị.
- Lưu riêng theo game, giữ trường `place` cũ và thêm `height` (pixel của cả dải khi mở, `null` là tự động), `icon` (cặp tọa độ tương đối với game, `null` nếu chưa dời icon). Cấu hình cũ vẫn đọc được; không lưu trạng thái đang chỉnh.

Thay đổi ở `src/overlay.rs`. Dịch vụ A.I.VOICE2, giao thức TTS, nhận diện game
và trích text dùng lại bản đã merge.

## Kiểm tra

```sh
cargo test --release --locked
python3 tools/uiux-smoke.py target/release/deps/vntext-8fa78a1d2254f22d --output target/diagnostics/layout-ui
cargo build --release --locked
git diff --check
```

Nếu Cargo đổi tên test binary, dùng đường dẫn được in bởi
`cargo test --release --locked --no-run`.

Smoke test chạy GTK thật trong KWin ảo riêng tại 1024×768 và 800×600,
dùng câu mẫu tự viết; không điều khiển desktop/game. Kiểm tra khung chờ câu,
bôi chọn qua sự kiện dịch/đổi nền, lịch sử giữ câu cũ, thu gọn/mở lại, vùng
nhận chuột ở chế độ chỉ bản dịch, vị trí lưu ngoài màn hình và năm tab thiết
lập. Phát tín hiệu gesture ngay trên widget trong KWin riêng để kiểm tra kéo
tiêu đề, hai góc đối diện, cạnh ngang, nhiều bước kéo liên tiếp, lưu/đọc lại,
khóa thao tác sau khi bấm Xong và cuộn câu dài trong chiều cao cố định.
Ảnh và log nằm ở thư mục output nói trên.

Kết quả ngày 2026-11-06: 133 test qua, 4 test mặc định bỏ qua;
smoke test overlay và launcher qua ở cả hai độ phân giải. Build release
và `git diff --check` qua. Đã xem ảnh của khung thường, bản dịch, lịch sử,
thu gọn, khung chờ câu và trường hợp chữ 72 px với bề rộng nhỏ.

### Sửa thao tác kéo ngày 2026-11-07

Đường cũ ghép vị trí cửa sổ vừa yêu cầu thay đổi với tọa độ gesture trong
widget đang di chuyển. Hình học cửa sổ và sự kiện chuột được xử lý không
đồng bộ, nên phép tính này có thể cộng lại chính chuyển động của khung,
tạo hiện tượng trôi khi người dùng giữ chuột.

Trong chế độ chỉnh, native surface giữ cố định theo màn hình và khung con
di chuyển bên trong. Gesture dùng vị trí sự kiện GDK trên native surface;
không cộng vị trí khung vào con trỏ. Vùng input chỉ bao khung đang hiện,
phần trong suốt ngoài khung cho click xuyên xuống game. Popup tra từ vẫn
tính theo khung con. Thoát chế độ chỉnh dùng lại native surface gọn theo dải.
Ngưỡng kéo 4 px loại click/rung tay nhẹ; giữ chuột không làm tiếp tục di
chuyển, click không ghi lại cấu hình vị trí.

135 unit test qua. Smoke test KWin riêng qua ở 1024×768 và 800×600,
bổ sung press đứng yên, rung tay 1 px, lặp update ở cùng điểm khi đang giữ,
resize hai góc, kiểm tra input ngoài khung và icon thu gọn/mở lại. Ảnh/log:
`target/diagnostics/stable-drag/`. Gesture tổng hợp trong test chỉ kiểm tra
logic và hình học; trải nghiệm chuột thật trên desktop cần chủ dự án thử lại.

### Vùng chữ và kéo icon thu nhỏ (2026-11-07)

`GtkFixed` đo khung con không theo cùng ràng buộc bề rộng như phần nội dung.
Vùng cuộn không có chiều cao tối thiểu rõ ràng có thể bị cấp một khe rất
thấp, dù dải đang ở chiều cao tự động. Đo phần điều khiển riêng, đo đoạn thoại
theo bề rộng thực sau padding, rồi đặt chiều cao viewport cụ thể. Chế độ tự
động giữ khoảng bốn dòng cho Nhật + dịch, hai dòng cho một ngôn ngữ; giãn đến
nội dung cần, giới hạn viewport ở 40% chiều cao màn hình hoặc 360 px. Chiều
cao cố định vẫn giữ tổng kích thước và cuộn phần thoại. Thông báo/lịch sử
nằm ngoài viewport. Padding 20 px ngang, 16 px dọc, khoảng cách Nhật/dịch
12 px; vị trí mặc định chừa 16 px hai mép game.

Khi thu nhỏ, native surface cũng giữ cố định và chỉ nhận input trên nút icon.
Gesture capture phân biệt click với kéo 4 px, claim khi kéo để hủy click
của button, lưu `icon` và giữ nguyên `place`/`height`. Nút có chặn thêm click
phát sinh cuối thao tác kéo. Giới hạn icon theo kích thước compact của nó;
khi mở, giới hạn dải theo kích thước đầy đủ. Đặt lại bố cục xóa cả `icon`.

Smoke test bổ sung câu Nhật và bản dịch nhiều dòng kèm thông báo HTTP 502,
kiểm tra cả hai label nằm trọn viewport; icon đứng yên, kéo, không tự mở,
lưu cấu hình, kéo sát góc và mở lại vẫn trong màn hình. Ảnh/log tại
`target/diagnostics/readable-panel/`.

Kiểm tra input Wayland thực (tùy chọn, cần `cc` và header `wayland-client`):

```sh
python3 tools/uiux-smoke.py target/release/deps/vntext-8fa78a1d2254f22d --pointer --output target/diagnostics/icon-pointer-final
```

Script biên dịch `tools/uiux-pointer.c`. Helper chỉ kết nối socket
`vntext-uiux-test` trong runtime `/tmp/vntext-uiux-*` với cờ test `isolated`;
từ chối môi trường desktop bình thường. KWin con riêng được cho phép nhận
giao thức test; không đổi cấu hình hay quyền của KWin desktop đang chạy.
Bài thử nhấn, di chuyển qua nhiều sự kiện, giữ rồi thả trên icon, xác nhận
icon dời đúng 60×40 px và không mở phụ đề. Phần input thực còn giúp phát hiện
callback leave đồng bộ khi ẩn vùng chữ; xử lý callback này không mượn lại
`Ui` đang được render để tránh panic. Toàn bộ unit test: 135 qua, 5 bỏ qua.

## Danh sách thử tay khi hỗ trợ hoặc cập nhật

1. Mở bản thử, chọn Ryujinx/Higurashi rồi quay lại game.
2. Nghe câu Nhật bằng nút loa; chuyển cả ba chế độ hiển thị.
3. Bôi chọn chữ và rê chuột tra từ; mở/đóng menu và thiết lập.
4. Thu gọn/mở lại; vào Chỉnh bố cục, kéo tiêu đề và các cạnh/góc, đổi độ đậm nền, bật/tắt cao tự động. Bấm Xong rồi mở lại phiên đọc để kiểm tra cấu hình đã lưu.
5. Xem lịch sử trong khi game chuyển câu, sau đó bấm về câu mới.
6. Nếu dùng nhiều màn hình, chuyển cửa sổ game sang màn hình khác và kiểm tra popup tra từ.

Chủ dự án đã duyệt trải nghiệm trong game. Kiểm tra nhiều màn hình thực chưa
có kết quả riêng; giữ mục này trong danh sách kiểm tra khi hỗ trợ.
