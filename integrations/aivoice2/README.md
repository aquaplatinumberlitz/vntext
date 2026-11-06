# Backend A.I.VOICE2 cho vntext và Anki

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
PYTHONPATH=/home/admin/aivoice2-re/src/daemon python3 integrations/aivoice2/server.py \
  --worker target/tts-runtime/engine_worker.exe
```

Đây là ví dụ cho máy dev hiện tại. Máy quản lý theo bộ cài phải thêm
`--config /đường/dẫn/runtime.json`, và PYTHONPATH trỏ tới daemon support
của bộ cài (runtime_config.py, run_auth_probe.py và dữ liệu liên quan).
Không chạy hai service trên cùng port/prefix. Worker mới và server mới phải
đi cùng nhau: server bật `AIVOICE2_WORKER_METRICS=1`, worker cũ không có
giao thức timing/chunk này. Worker gốc không bị chép đè.

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
hủy queued job hoặc dừng âm thanh. Cache trên đĩa kế thừa backend cũ chưa
có quota; RAM vntext có giới hạn riêng 32 MiB/8 câu/120 giây.

## Service trên máy này và quay lại bản cũ

Đang có drop-in riêng:
`~/.config/systemd/user/aivoice2-tts.service.d/50-vntext-latency.conf`.
Sau merge, ExecStart trỏ source trong `/home/admin/Projects/vntext` (main)
và worker dưới `target/tts-runtime` của cùng checkout;
PYTHONPATH trỏ daemon support gốc. Port vẫn 17862, dùng dữ liệu và prefix
đã cài. Source/binary worker gốc trong aivoice2-re giữ nguyên.

Muốn rollback, đóng client đang phát rồi:

```bash
systemctl --user stop aivoice2-tts
python3 - <<'PY'
from pathlib import Path
link = Path('/home/admin/aivoice2-re/wineprefix/dosdevices/r:')
owned = {
    Path('/home/admin/Projects/vntext/target/tts-runtime').resolve(),
    Path('/home/admin/Projects/vntext-uiux/target/tts-runtime').resolve(),
}
original = Path('/home/admin/aivoice2-re/src/daemon/build').resolve()
if not link.is_symlink() or link.resolve() not in owned | {original}:
    raise SystemExit('Runtime alias không thuộc bản đã thiết lập; kiểm tra trước khi đổi.')
if link.resolve() != original:
    link.unlink()
    link.symlink_to(original, target_is_directory=True)
PY
rm ~/.config/systemd/user/aivoice2-tts.service.d/50-vntext-latency.conf
systemctl --user daemon-reload
systemctl --user restart aivoice2-tts
```

Ví dụ này chỉ dành cho máy dev có đường dẫn/prefix nêu trên. Alias R: phải
được trả về thư mục worker gốc; loader cố ý không tự ghi đè alias khác đích.
Chỉ bỏ drop-in này, không dùng `systemctl revert` vì có thể xóa cấu hình
khác của người dùng. vntext sẽ dùng lại đường WAV tương thích.
