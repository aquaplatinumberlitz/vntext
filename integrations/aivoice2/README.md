# Backend đã chuyển sang aivoice2api

Nguồn chính: [aquaplatinumberlitz/aivoice2api](https://github.com/aquaplatinumberlitz/aivoice2api).

- [Kiến trúc ba repo](https://github.com/aquaplatinumberlitz/aivoice2api/blob/main/docs/ARCHITECTURE_VI.md)
- [Kết nối và vận hành runtime chung](https://github.com/aquaplatinumberlitz/aivoice2api/blob/main/docs/CONNECT_VI.md)
- [Hợp đồng API](https://github.com/aquaplatinumberlitz/aivoice2api/blob/main/docs/API_VI.md)
- [Build/release backend](https://github.com/aquaplatinumberlitz/aivoice2api/blob/main/docs/RELEASE_VI.md)
- [Client vntext](../../docs/TTS_CLIENT_VI.md)

Source worker/server/scheduler/cache và test Python đã chuyển sang repo đó.
Thư mục này giữ link cũ; không chứa backend để build/chạy từ checkout vntext.
`aivoice2api.lock.json` ở gốc vntext ghi phiên bản backend tham chiếu cho QA.
Lịch sử đo/test trước khi tách vẫn ở docs/VNTEXT_TTS_PLAN.md và Git.
