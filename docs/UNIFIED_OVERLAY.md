# Dải nổi hợp nhất

Nhánh triển khai: `feat/unified-overlay`, dựa trên `main` tại `02de868`.
Ngày 2026-11-06, chủ dự án đã thử bản mới, xác nhận "ok rồi" và duyệt merge/push.

## Bố cục

- Một khung nền mờ chứa hàng điều khiển, câu Nhật với nút loa, bản dịch và lịch sử.
- Hàng điều khiển dịu đi khi chuột rời khung; chiều cao không đổi.
- Thu gọn thành **vntext · Hiện** ở vị trí của khung; bấm để mở lại.
- Lịch sử mặc định chỉ có một nút nhỏ. Mở ra mới hiện trước/sau, vị trí và nút về câu mới.
- Mặc định nằm dưới game, thiếu chỗ thì nằm trong sát đáy game. Giới hạn vị trí theo màn hình chứa tâm game.
- **⋯ → Chỉnh bố cục…** hiện viền, tám tay nắm resize và vùng kéo trên tiêu đề. Bấm **Xong** để khóa lại; chữ Nhật vẫn chọn/tra được.
- Chỉnh **Độ đậm nền** 20–100%, xem trực tiếp kèm phần trăm. Chỉ nền đổi độ đậm.
- Resize ngang giữ chế độ cao hiện tại; resize dọc/góc chuyển sang cao cố định. Bật **Chiều cao tự động** để theo nội dung. Câu dài cuộn trong khung; tiêu đề luôn ngoài vùng cuộn.
- **Đặt lại** khôi phục vị trí/kích thước/độ đậm nền; giữ cỡ chữ, màu và chế độ hiển thị.
- Lưu riêng theo game, giữ trường `place` cũ và thêm `height` (pixel của cả dải khi mở, `null` là tự động). Cấu hình cũ không có trường này vẫn dùng chiều cao tự động. Không lưu trạng thái đang chỉnh.

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

## Danh sách thử tay khi hỗ trợ hoặc cập nhật

1. Mở bản thử, chọn Ryujinx/Higurashi rồi quay lại game.
2. Nghe câu Nhật bằng nút loa; chuyển cả ba chế độ hiển thị.
3. Bôi chọn chữ và rê chuột tra từ; mở/đóng menu và thiết lập.
4. Thu gọn/mở lại; vào Chỉnh bố cục, kéo tiêu đề và các cạnh/góc, đổi độ đậm nền, bật/tắt cao tự động. Bấm Xong rồi mở lại phiên đọc để kiểm tra cấu hình đã lưu.
5. Xem lịch sử trong khi game chuyển câu, sau đó bấm về câu mới.
6. Nếu dùng nhiều màn hình, chuyển cửa sổ game sang màn hình khác và kiểm tra popup tra từ.

Chủ dự án đã duyệt trải nghiệm trong game. Kiểm tra nhiều màn hình thực chưa
có kết quả riêng; giữ mục này trong danh sách kiểm tra khi hỗ trợ.
