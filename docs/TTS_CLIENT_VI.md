# Client TTS vntext

**Cài từ đầu: [Quickstart chung](https://github.com/aquaplatinumberlitz/aivoice2-anki/blob/main/docs/QUICKSTART_VI.md).**
Bộ cài Anki đã kèm backend mới; vntext chỉ gọi cùng URL. Nếu chưa build
client, xem [INSTALL_VI](INSTALL_VI.md). Trang này giải thích cấu hình và
hành vi TTS khi đã có backend, không yêu cầu build lại engine.

## Nối backend đã có

1. Mở vntext/game theo [hướng dẫn sử dụng](USER_GUIDE_VI.md), vào dải → Giọng đọc → Kết nối nâng cao.
2. Nhập URL đúng bản cài, bấm **Làm mới giọng** để áp dụng và lưu; chọn giọng.
3. **Nghe thử**, rồi nghe câu Nhật/đoạn chọn từ dải. Mặc định tự đọc tắt.
4. Anki và vntext phải cùng URL/instance nếu muốn dùng chung runtime.

Cấu hình `${XDG_CONFIG_HOME:-$HOME/.config}/vntext/tts.json` giữ endpoint và
lựa chọn theo game. Không thay cả config bằng ví dụ rỗng làm mất lựa chọn.
Client thương lượng capabilities; backend WAV cũ vẫn hoạt động. Lock repo
là phiên bản QA tham chiếu, không ép backend tự update khi mở vntext.

## Hành vi và giới hạn

- Một câu pending mới nhất; session/revision giúp loại yêu cầu cũ của đúng
  phiên. Client dừng tiếng cũ khi đổi câu, nhưng native synth đang chạy có
  thể phải hoàn tất trước khi câu mới được tổng hợp.
- Prepare câu hiện tại tối đa 160 ký tự khi bật prepare, tự đọc tắt và game
  có focus. Sau khi bắt đầu nghe, lookahead tối đa 1 đoạn index+1 ngắn.
  Không phân tích đồ thị nhánh, OCR không có đoạn sau; không prefetch menu ▶.
- Streaming PCM tới PipeWire; WAV cache ở backend. vntext không ghi WAV
  tạm ra đĩa; RAM nghe lại giới hạn 32 MiB/120 giây, tối đa 7 bản ghi hiện
  thực. Hết 120 giây là điều kiện không tái dùng, không timer xóa RAM ngay.
- Backend có quota WAV riêng 512 MiB/7 ngày; quota này không tính RAM
  engine/player. Xem API backend để hiểu cache lease và lịch dọn.
- Chỉ cache stream có frame kết thúc thành công; HTTP đóng sớm không đủ.
  Một phần tiếng có thể đã phát trước khi phát hiện lỗi ở cuối stream.

## Test khi sửa client

```bash
cargo test --release --locked tts
```

Test unit không cần game, loa, Wine hoặc translation API. Test live/UI là
một bước riêng: chủ máy mở game, chọn câu mẫu, chuyển câu/giọng và thử
preview/dừng/cache. Không tự mở game, giả chuột/phím hoặc dùng key dịch
để chạy test. Quy tắc làm việc vẫn ở PLAN mục 1–3.

Trống câu Nhật khi mở lại game: xem [engines.md](engines.md), đặc biệt
phần shin/Umineko/Higurashi; đây có thể là extraction, không phải backend.
Backend ready và WAV hợp lệ nhưng vntext im: kiểm pw-play/PipeWire, output
thiết bị, capability và trạng thái generation của câu trước khi sửa worker.
