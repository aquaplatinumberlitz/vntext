# vntext trong kiến trúc ba repo

Kiến trúc chung hiện hành tại [aivoice2api](https://github.com/aquaplatinumberlitz/aivoice2api/blob/main/docs/ARCHITECTURE_VI.md).
Repo vntext sở hữu extraction/OCR, dịch/tra từ, launcher/overlay và client TTS.
Server, worker Wine, runtime config/CLI, cache WAV và test backend thuộc
`aivoice2api`. Addon, bộ cài/quản lý và GUI patch thuộc `aivoice2-anki`.

| Source vntext | Trách nhiệm |
|---|---|
| `src/engines/`, `src/mem.rs`, `src/main.rs` | Nhận diện game, text Nhật và vị trí câu |
| `src/ocr.rs`, `screen.rs`, `kwin.rs` | OCR/cửa sổ/focus và ảnh theo desktop |
| `src/overlay.rs` | UI, chọn câu/giọng, trigger prepare/lookahead và playback |
| `src/tts.rs` | HTTP, capability negotiation, session/revision, latest pending và cache nghe lại |
| `src/tts_audio.rs` | Player PipeWire/PCM và dừng đúng tiến trình |
| `aivoice2api.lock.json` | Phiên bản backend tham chiếu cho QA; không cài backend vào vntext |

Client không yêu cầu mở Anki/Editor. Một backend đã cài độc lập cung cấp
cùng URL cho cả hai ứng dụng. vntext không đọc license hoặc spawn engine;
không tự thay service/backend khi build hoặc cập nhật vntext.

Sau khi tách, `integrations/aivoice2/README.md` chỉ chuyển hướng. Source/test
backend đã chuyển sang aivoice2api; Git vẫn giữ lịch sử trước đó. Các plan
UI/TTS ghi nhánh/worktree cũ là hồ sơ triển khai, không là lệnh vận hành mới.

Thay API: phát hành backend giữ tương thích rồi cập nhật lock/test client.
Thay UI/player/lookahead: sửa tại vntext, không cần fork backend. Chi tiết
kết nối/test và ranh giới cache ở [TTS_CLIENT_VI.md](TTS_CLIENT_VI.md).
