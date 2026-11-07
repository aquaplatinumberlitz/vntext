# Tài liệu vntext

**Lần đầu nối A.I.VOICE2 + Anki + vntext: [Quickstart chung](https://github.com/aquaplatinumberlitz/aivoice2-anki/blob/main/docs/QUICKSTART_VI.md).**
Chỉ cần vntext độc lập thì bắt đầu tại INSTALL_VI; không cần backend TTS.

| Bạn cần | Tài liệu hiện hành |
|---|---|
| Distro/dependency, Rust/build, cài vào menu | [INSTALL_VI](INSTALL_VI.md) |
| Dải, dịch, tra từ, OCR, lệnh và game đã hỗ trợ | [USER_GUIDE_VI](USER_GUIDE_VI.md) |
| URL/giọng, streaming, prepare/lookahead/cache RAM | [TTS_CLIENT_VI](TTS_CLIENT_VI.md) |
| Sửa source hoặc thêm profile | [DEVELOPMENT_VI](DEVELOPMENT_VI.md) |
| Hiểu ba repo và ranh giới backend/client | [ARCHITECTURE_VI](ARCHITECTURE_VI.md) |
| Định dạng engine và backlog shin/Higurashi | [engines](engines.md) |
| Bố cục/resize/opacity đã triển khai | [UNIFIED_OVERLAY](UNIFIED_OVERLAY.md) |

## Hồ sơ triển khai và link cũ

| Tài liệu | Phạm vi |
|---|---|
| [PLAN](PLAN.md) | Mục 1–3 vẫn là quy tắc làm việc; các giai đoạn cũ là hồ sơ |
| [UIUX_REVIEW_PLAN](UIUX_REVIEW_PLAN.md) | Lịch sử UI đã merge |
| [VNTEXT_TTS_PLAN](VNTEXT_TTS_PLAN.md) | Benchmark/test/lịch sử trước khi tách backend; không dùng đường dẫn triển khai cũ |
| [AIVOICE2_CONNECT_VI](AIVOICE2_CONNECT_VI.md) | Chuyển hướng tới guide kỹ thuật chung |

Backend/API/config/WAV quota/build/release chỉ duy trì ở
[aivoice2api/docs](https://github.com/aquaplatinumberlitz/aivoice2api/tree/main/docs).
Addon/bộ cài ở [Anki docs](https://github.com/aquaplatinumberlitz/aivoice2-anki/tree/main/docs).
`integrations/aivoice2/README.md` giữ link cũ; source/test backend đã chuyển.
Đường dẫn worktree trong plan thuộc lịch sử, không là đường dẫn máy khách.
