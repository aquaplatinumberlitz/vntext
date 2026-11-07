# Cài vntext từ source

Chỉ cài client trích text/phát audio ở đây. Nếu muốn nối cả Anki và
A.I.VOICE2, làm [Quickstart chung](https://github.com/aquaplatinumberlitz/aivoice2-anki/blob/main/docs/QUICKSTART_VI.md)
trước; bộ cài Anki đã có backend mới. Không build worker Wine trong repo này.

## 1. Kiểm máy có đáp ứng không

```bash
cat /etc/os-release
printf 'Desktop session: %s\n' "$XDG_SESSION_TYPE"
```

Dải nổi cần **Wayland + layer-shell**: KDE Plasma, Sway hoặc Hyprland.
GNOME không có layer-shell; có thể dùng dòng lệnh/trang web nhưng không
có dải nổi. Game thường chạy X11 qua XWayland; Wine Wayland native chỉ có
đường hỗ trợ riêng trên KDE. Cần GTK **4.10 trở lên** theo Cargo.toml;
Ubuntu 22.04 với GTK mặc định không đáp ứng. Ubuntu 24.04 có GTK đủ mới
nhưng **chưa có libgtk4-layer-shell-dev trong kho gói chuẩn**, nên không
thuộc luồng cài gói đơn giản bên dưới. Dùng Arch/CachyOS, Ubuntu 26.04 trở
lên hoặc Debian 13; distro khác cần kiểm hai thư viện trước khi tiếp tục.

## 2. Cài dependency cho distro của bạn

Chỉ chạy nhóm đúng distro. Các lệnh sudo cài gói và hỏi mật khẩu; build/
chạy vntext sau đó bằng tài khoản desktop thường, không dùng sudo.

Arch / CachyOS / EndeavourOS:

```bash
sudo pacman -Syu --needed base-devel pkgconf git rustup gtk4 gtk4-layer-shell wl-clipboard pipewire-audio xorg-xwayland
rustup default stable
```

Ubuntu 26.04 trở lên / Debian 13:

```bash
sudo apt update
sudo apt install build-essential pkg-config git curl libgtk-4-dev libgtk4-layer-shell-dev wl-clipboard pipewire-bin xwayland
```

Trên Ubuntu/Debian, cài Rust stable bằng hướng dẫn tại [rustup.rs](https://rustup.rs/)
trước khi sang bước 3; chọn cài mặc định, mở terminal mới sau khi hoàn tất.
Không thay bằng rustc/cargo quá cũ từ distro nếu dependency báo MSRV.

Kiểm dependency trước khi build:

```bash
rustc --version
cargo --version
pkg-config --modversion gtk4 gtk4-layer-shell-0
command -v pw-play wl-copy
```

Mong đợi có version Rust/Cargo, GTK ít nhất 4.10, layer-shell **cho GTK4** và
hai lệnh có đường dẫn. Gói gtk-layer-shell dành cho GTK3 không thay được
`gtk4-layer-shell`. Nếu apt không có gói hoặc version quá thấp, dùng distro
có bản phù hợp thay vì tiếp tục build; đừng tự ghi đè GTK hệ thống.
Đã đối chiếu tên gói layer-shell trong [Ubuntu packages](https://packages.ubuntu.com/search?keywords=gtk4-layer-shell&searchon=names&suite=all&section=all)
và [Debian packages](https://packages.debian.org/search?keywords=gtk4-layer-shell&searchon=names&suite=all&section=all).
Lệnh gói được kiểm cú pháp, chưa phải chứng nhận build trên mọi distro.

## 3. Lấy source và build một lần

Mở terminal tại thư mục bạn muốn lưu source lâu dài. Nếu `vntext` đã tồn
tại, vào đúng checkout đó thay vì clone đè.

```bash
git clone https://github.com/aquaplatinumberlitz/vntext.git
cd vntext
cargo build --release --locked
./target/release/vntext install
./target/release/vntext
```

**Xong bước này:** cửa sổ chọn game mở, menu ứng dụng có vntext. Build lần
đầu tải dependency và có thể lâu; không cần mở game để build. Giữ thư mục
source/build vì shortcut trỏ tới binary trong target/release. Nếu dời thư
mục, chạy lại lệnh `./target/release/vntext install` tại vị trí mới.

## 4. Kiểm game và nối giọng đọc

Bạn mở game tới chỗ có thoại, chọn game → **Hiện phụ đề**. Nếu tool báo
không đọc được bộ nhớ, xem khung **Chi tiết kiểm tra** / nút **Cho phép**.
Nút này hỏi mật khẩu và thay chính sách ptrace của hệ thống để các process
cùng user đọc bộ nhớ nhau; chỉ bật khi bạn chấp nhận thay đổi đó. Không cần
tắt quyền bảo vệ này chỉ để kiểm backend/Nghe thử TTS.

Để nghe: **Giọng đọc → Kết nối nâng cao** → URL thật từ bộ cài Anki →
**Làm mới giọng** → chọn giọng → **Nghe thử**. Xem [TTS_CLIENT_VI](TTS_CLIENT_VI.md)
hoặc quay lại [Quickstart chung bước 5](https://github.com/aquaplatinumberlitz/aivoice2-anki/blob/main/docs/QUICKSTART_VI.md#5-cài-vntext-và-nối-cùng-backend).
Anki/Editor không cần mở và không cần service TTS thứ hai.

## Nếu chưa chạy được

| Lỗi | Kiểm tra |
|---|---|
| cargo/rustc: command not found | Cài Rust stable, mở terminal mới; trên Arch đã chạy rustup default stable chưa? |
| pkg-config không thấy gtk4 / layer-shell | Đúng gói GTK4, đúng distro/repository và phiên bản tối thiểu? |
| Linker cc không có | Cài base-devel hoặc build-essential |
| Không có dải nổi | Phiên Wayland có layer-shell? Kiểm `./target/release/vntext doctor` |
| Không tìm thấy game / chưa nhận diện | Game có cửa sổ, đã nạp thoại? Bấm Nhận diện lại và xem [game đã hỗ trợ](USER_GUIDE_VI.md#game-đã-hỗ-trợ) |
| Nghe thử backend được nhưng không có câu Nhật | Khoanh vùng extraction/profile theo [engines.md](engines.md), không sửa worker TTS |
| Không phát TTS | Kiểm URL thật, backend ready, giọng còn được cài, pw-play và thiết bị âm thanh |

Dịch/OCR và từ điển là bước cấu hình riêng sau khi tool mở được. Không cần
API key dịch hoặc OCR để build, test câu mẫu TTS hay đọc game có profile.
Cách dùng tiếp: [USER_GUIDE_VI](USER_GUIDE_VI.md).
