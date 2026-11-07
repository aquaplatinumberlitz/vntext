# vntext

Đọc tiếng Nhật từ visual novel trên Linux, hiện phụ đề/dịch, tra từ và
phát giọng A.I.VOICE2 ngay trên dải nổi.

**Mới cài A.I.VOICE2 + Anki + vntext? → [Bắt đầu A–Z tại đây](https://github.com/aquaplatinumberlitz/aivoice2-anki/blob/main/docs/QUICKSTART_VI.md).**
Bộ cài Anki đã kèm backend mới. Bạn không cần build aivoice2api hoặc tạo
thêm service cho vntext. vntext cũng dùng độc lập nếu không cần TTS.

## Cài và dùng

1. [Cài vntext](docs/INSTALL_VI.md): Linux/Wayland, gói hệ thống, Rust, build và menu.
2. Bạn mở game tới câu thoại → mở vntext → chọn game → **Hiện phụ đề**.
3. Muốn nghe A.I.VOICE2: nhập URL backend trong **Giọng đọc → Kết nối nâng cao**,
   **Làm mới giọng**, chọn giọng → **Nghe thử**.

vntext hiện cần build từ source; chưa có gói binary cho newbie. Overlay
cần Wayland có layer-shell (KDE Plasma/Sway/Hyprland). Hướng dẫn cài ghi rõ
phiên bản GTK và các trường hợp distro không đáp ứng.

## Đọc tiếp theo việc cần làm

| Bạn cần | Tài liệu |
|---|---|
| Dải nổi, dịch, tra từ, OCR, lệnh và game đã hỗ trợ | [Hướng dẫn sử dụng](docs/USER_GUIDE_VI.md) |
| Giọng đọc, tự đọc, cache và xử lý lỗi TTS | [Client TTS](docs/TTS_CLIENT_VI.md) |
| Hiểu source, sửa profile hoặc thêm engine | [Phát triển](docs/DEVELOPMENT_VI.md), [ghi chú engine](docs/engines.md) |
| Kiến trúc ba repo và trách nhiệm | [Kiến trúc](docs/ARCHITECTURE_VI.md) |
| Tìm mọi tài liệu và hồ sơ UI/TTS cũ | [Chỉ mục](docs/README.md) |

## Ba repo liên quan ra sao?

| Repo | Dùng khi nào |
|---|---|
| [aivoice2-anki](https://github.com/aquaplatinumberlitz/aivoice2-anki) | Tải bộ cài backend + addon; nhận MSI/Lang/Voice riêng từ bên bàn giao |
| [aivoice2api](https://github.com/aquaplatinumberlitz/aivoice2api) | Dev sửa/build backend hoặc tra API; người cài thường không cần clone |
| vntext | Build client này, chọn game và phát câu Nhật qua cùng backend |

Backend/Anki là repo private; nếu link 404 cần quyền đọc hoặc gói được bàn
giao trực tiếp. Code vntext không kèm voicebank hoặc engine A.I.VOICE2.

Cấu hình và cache nằm ngoài repo ở ~/.config/vntext, ~/.cache/vntext và
~/.local/share/vntext. Không đưa thoại game, key dịch hoặc dữ liệu cá nhân
vào commit/báo cáo hỗ trợ. Khi phát triển, đọc [quy tắc làm việc](docs/PLAN.md).
