# Tài liệu vntext

Cập nhật 2026-11-07: vntext là client/extractor; backend TTS có nguồn riêng.

| Nhu cầu | Tài liệu | Trạng thái |
|---|---|---|
| Cài/build, sử dụng launcher, overlay và mở rộng profile | [README](../README.md) | Hướng dẫn chính của sản phẩm |
| Ranh giới source giữa ba repo | [ARCHITECTURE_VI](ARCHITECTURE_VI.md) | Hiện hành |
| Nối backend, playback/lookahead/cache và test client | [TTS_CLIENT_VI](TTS_CLIENT_VI.md) | Hiện hành |
| Định dạng/profile game và backlog kiểm tra shin/Higurashi | [engines](engines.md) | Ghi chú kỹ thuật + ca chưa nghiệm thu |
| Layout/resize/opacity và thao tác dải/icon thu nhỏ | [UNIFIED_OVERLAY](UNIFIED_OVERLAY.md) | Thiết kế đã triển khai; lệnh smoke có thể theo build cũ |
| Quy tắc làm việc và lịch sử các giai đoạn newbie | [PLAN](PLAN.md) | Mục 1–3 vẫn là quy tắc; phần triển khai/lộ trình là hồ sơ |
| Các bước UI trước khi thêm TTS | [UIUX_REVIEW_PLAN](UIUX_REVIEW_PLAN.md) | Lịch sử đã merge |
| Kết quả đo/test và quá trình tối ưu TTS trước khi tách | [VNTEXT_TTS_PLAN](VNTEXT_TTS_PLAN.md) | Lịch sử; không dùng đường dẫn/backend cũ để triển khai |
| Link guide chung từng gửi | [AIVOICE2_CONNECT_VI](AIVOICE2_CONNECT_VI.md) | Chuyển hướng, không giữ bản sao |

API, worker, quota WAV, cấu hình runtime, build/release và runbook chung
chỉ duy trì ở [aivoice2api/docs](https://github.com/aquaplatinumberlitz/aivoice2api/tree/main/docs).
Addon/bộ cài ở [Anki docs](https://github.com/aquaplatinumberlitz/aivoice2-anki/tree/main/docs).
`integrations/aivoice2/README.md` giữ link cũ; source/test backend đã chuyển.

Giữ mốc đo và giới hạn QA trong tài liệu lịch sử; không đổi số liệu cũ
thành cam kết bản hiện tại. Các đường dẫn worktree trong plan không phải
bố trí cài đặt máy khách. Repo chính hiện ở `/mnt/m2sata/A.I.VOICE2-futago/vntext`.
