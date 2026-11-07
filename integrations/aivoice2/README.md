# Backend A.I.VOICE2 cho vntext và Anki

Dev mới: bắt đầu với [hướng dẫn kết nối đầy đủ](../../docs/AIVOICE2_CONNECT_VI.md).

Chủ dự án đã nghiệm thu bản UI/TTS tối ưu và duyệt merge ngày 2026-11-06.

Source nền lấy từ `/home/admin/aivoice2-re/src/daemon/` ngày 2026-11-06;
chỉ thay giao thức worker, đo thời gian, lịch tác vụ và vận chuyển audio.
Giữ nguyên các lời gọi khởi tạo/authentication/voice/dialect của engine.
Không sửa binary engine hay license trong đợt tối ưu này.

## Build và chạy

Từ thư mục repo vntext:

```bash
mkdir -p target/tts-runtime
x86_64-w64-mingw32-gcc -Wall -Wextra -Werror -O2 -Wl,--no-insert-timestamp \
  integrations/aivoice2/engine_worker.c -o target/tts-runtime/engine_worker.exe
```

Sau build, triển khai đồng bộ server/worker vào runtime theo mục 8 trong
hướng dẫn kết nối. Service dùng `runtime/server.py --config ...`, không
cần chạy server từ checkout. `--worker` chỉ dành cho config/prefix dev riêng:
đổi runtime_dir trên prefix đã map R: sang đích khác sẽ bị từ chối.
Nếu chạy source trực tiếp với config dev, PYTHONPATH phải trỏ tới daemon
support của bộ cài (runtime_config.py, run_auth_probe.py và dữ liệu liên quan).
Không chạy hai service trên cùng port/prefix. Worker mới và server mới phải
đi cùng nhau: server bật `AIVOICE2_WORKER_METRICS=1`, worker cũ không có
giao thức timing/chunk này. Giữ backup đồng bộ trước khi thay runtime đã cài.

Test không dùng engine thật:

```bash
python3 tests/test_tts_backend.py
```

## Hợp đồng bổ sung

- `/v1/speak` giữ JSON `{text,voice}` và trả WAV như Anki cũ. Server-Timing
  có queue_ms/load_ms/first_pcm_ms/synth_ms/native_ms khi tổng hợp mới.
- `/healthz` thêm capabilities. Client cũ bỏ qua các field mới; client
  vntext tự lùi về WAV nếu backend không có `pcm-stream-v1`.
- `/v1/speak/stream`: JSON như speak, thêm session/revision tùy chọn.
  Trả `application/x-aivoice2-pcm`, prefix `AVS1`, sau đó frame
  `u8 kind + u32le size + payload`. Kind 1: PCM16 little endian mono 48 kHz;
  kind 2: JSON kết thúc thành công (cache/timing); kind 3: JSON lỗi.
  Không nhận kết thúc HTTP làm bằng chứng thành công. Chunk PCM tối đa 64 KiB.
- `/v1/prepare`: session/revision/voice/mode bắt buộc theo hợp đồng client;
  mode `warm` chỉ nạp/chọn Lang và voicebank, không tổng hợp câu giả hoặc
  dùng cache WAV. Mode `text` tổng hợp không phát, giới hạn 160 ký tự.
  Trả 202 accepted; đây là xác nhận xếp việc, chưa phải đã tạo xong audio.
- `/v1/cancel`: session/revision, xóa preparation chưa chạy và foreground
  revision cũ đang chờ của đúng session.
  Revision cũ hơn bị bỏ. Client không được dùng chung session giữa hai phiên.

Một worker native, foreground FIFO có ưu tiên hơn preparation. Tối đa 24
foreground chờ và 24 session preparation, mỗi session chỉ một việc mới
nhất. Preparation hết hạn sau 8 giây chờ. Foreground chờ tối đa 45 giây;
không hứa fairness cho preparation khi foreground liên tục bận.

Cache hit tra theo khóa text/voice/engine/worker/assets và trả ngay ngoài
khóa tổng hợp. WAV được publish bằng rename sau kiểm tra, không trả file
chưa viết xong. Server stream nhận PCM và vẫn tạo cache WAV song song.
Mỗi HTTP stream có bộ đệm tối đa 16 MiB; client chậm/mất kết nối không
chặn native worker. Audio lỗi/truncated có thể đã phát một phần trước khi
được phát hiện; kết quả đó không được client đưa vào cache.

Preparation **đang chạy trong DLL không bị hủy**; có thể làm câu foreground
vừa đến phải chờ phần còn lại. Đó là giới hạn hiện tại, không đánh đồng với
hủy queued job hoặc dừng âm thanh.

## Cache WAV và chuẩn bị trước

Cache backend giới hạn **512 MiB**, loại file ít dùng gần đây nhất khi vượt
quota và dọn WAV không được dùng trong **7 ngày**. Dọn lúc khởi động, sau khi
publish audio mới và mỗi 60 giây khi service chạy. Cache hit cập nhật mtime
để thứ tự sử dụng vẫn đúng trên filesystem `noatime` và sau khi restart.
Chỉ dọn file thường có tên `aivoice2_<64 ký tự hex>.wav` trong cache_dir;
không quét đệ quy, không theo symlink. Temp đang ghi dùng tên khác.

Lease bảo vệ file đang tổng hợp và đang trả WAV/PCM. Nếu file được giữ làm
quota tạm vượt mức, dọn lại khi lease cuối đóng. Cache hit không chờ native
lock; việc dọn cache dùng khóa riêng. `/healthz.wav_cache` báo quota, tuổi
hết hạn, số file/byte ở lần dọn gần nhất, tổng số đã xóa và trạng thái hoãn.
Audio vẫn stream PCM sang vntext trong lúc worker ghi WAV cache song song.

Giữ **1 đoạn kế tiếp, tối đa 160 ký tự** làm mặc định cho visual novel nhiều
nhánh. Đây là dự đoán index+1, chưa phải phân tích đồ thị nhánh của kịch bản;
không chuẩn bị menu lựa chọn bắt đầu bằng ▶ và OCR không có lookahead.
Revision mới thay/hủy pending cũ khi chuyển câu/load save/đổi giọng; tác vụ
native đã chạy vẫn không hủy được. Vì vậy chưa tăng lên 3–5 đoạn hoặc tổng
hợp trước tất cả nhánh. Mức 2 chỉ là hướng thử sau khi có số liệu độ trễ,
tỷ lệ dùng lại dự đoán và khi profile biết đường đi tiếp là tuyến tính.

RAM vntext giữ quota **32 MiB / 120 giây**. Số bản ghi hiện thực tối đa 7
(6 lịch sử và 1 gần nhất); các ghi chú cũ nói 8 là mức dự kiến. RAM này phục
vụ nghe lại; audio chuẩn bị trước hiện được giữ ở WAV cache backend. Tăng
RAM quota riêng không làm tăng lookahead. 120 giây là điều kiện tái dùng;
không có timer RAM riêng để giải phóng ngay lúc hết hạn.

## Runtime chung trên máy này (2026-11-07)

Anki và vntext cùng dùng **http://127.0.0.1:17863**. Một service duy nhất:
`aivoice2-local-df97fb54247a.service`; app, Voice, Lang, prefix và config nằm
trong `~/.local/share/aivoice2-local`. Không còn phụ thuộc thư mục nghiên cứu
`/home/admin/aivoice2-re`. Runtime được triển khai từ source tích hợp vntext,
không cần mở vntext hoặc Anki để service sẵn sàng.

Source `server.py` nhận `--config` và dùng worker trong runtime_dir của
config; `--worker` là override tùy chọn cho dev. Vì vậy service do manager
sinh lại bằng runtime_cli vẫn gọi được bản streaming/cache quota mới.
Anki giữ nguyên endpoint 17863; vntext đã được chuyển từ 17862 sang 17863.

Đợt chuyển có backup file runtime/manifest trước nâng cấp và receipt hash
trong `backups/integration/shared-tts-*/`. Hash install-manifest được cập
nhật theo đúng các file được nâng cấp có chủ đích; không sửa hash để che
lỗi file. Dùng `runtime_cli.py --config ... verify-files` để kiểm tra.

Các unit `aivoice2-tts.service` cũ và `aivoice2-local-e99b93850a9a.service`
thử đã dừng và bỏ tự khởi động. Giữ shortcut bản Anki chính và quản lý;
xóa các shortcut thử/trùng. Bản Editor Wine cá nhân ở `~/.wine-aivoice`
không bị xóa.

Để rollback, dừng service chính, khôi phục đồng bộ server/worker và các
manifest từ cùng thư mục backup migration rồi khởi động lại service. Bản
cũ chỉ có WAV, vntext tự lùi về giao thức đó. Không chạy worker mới với
server cũ hoặc ngược lại; không dùng lại unit/prefix trong thư mục nghiên
cứu đã xóa.

Bộ bàn giao 0.2.0 cùng checksum/QA được giữ ở
`~/Downloads/aivoice2-handover/`; bên trong archive có developer-kit và tài
liệu RE. Gói này giữ bản phát hành Anki cũ, không tự bao gồm cập nhật
streaming/cache quota riêng của vntext.

Tests backend lấy runtime support tại `~/.local/share/aivoice2-local/runtime`
hoặc biến `AIVOICE2_SUPPORT_DIR`; không yêu cầu đường dẫn nghiên cứu cũ.
