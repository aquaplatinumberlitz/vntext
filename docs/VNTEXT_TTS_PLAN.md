# Pipeline vntext → A.I.VOICE2

Ngày 2026-11-06: chủ dự án xác nhận bản tối ưu hoạt động và duyệt merge
`feat/uiux-before-tts` vào main. Các mốc chưa commit/chưa nghiệm thu bên
dưới mô tả lịch sử từng bước, không phải trạng thái bàn giao hiện tại.

Ngày 2026-11-06, branch `feat/aivoice2-tts`, từ commit `4f49e33`.
Đây là kế hoạch mở rộng riêng; các giai đoạn của `PLAN.md` là kế hoạch
giao diện cho người mới. Bước khởi động ban đầu chưa triển khai TTS.

## Triển khai trên nhánh UI mới

Chủ dự án đã nghiệm thu việc hiện dải/nhận diện Higurashi và duyệt tiếp tục
TTS trên `feat/uiux-before-tts`, worktree `/home/admin/Projects/vntext-uiux`.
Không merge hoặc commit trong giai đoạn này.

### Đã làm

Các đoạn dưới mô tả phiên bản WAV ban đầu; đợt tối ưu được duyệt và triển
khai tiếp được ghi ở mục "Tối ưu live TTS" bên dưới.

- `src/tts.rs`: client HTTP cục bộ, timeout, giới hạn text 4.000 ký tự và
  audio 16 MiB, kiểm tra RIFF/WAV PCM16 mono 48 kHz. Không dùng proxy môi
  trường hoặc theo HTTP redirect để gọi backend local.
- Một worker synthesis cho mỗi reader overlay, một ô pending mới nhất.
  Bộ nhận sự kiện riêng tiếp tục chạy trong lúc HTTP chờ. Generation ngăn
  audio cũ phát sau chuyển câu/dừng/đổi giọng/tắt tự đọc. Không tạo worker
  trong launcher, không khởi động thêm engine/Wine hoặc sửa API Anki.
- Nhận message hoàn chỉnh từ profile/OCR; chỉ đọc `lines`, tách tên nhân vật;
  bỏ sự kiện dịch, thông báo, text trùng liên tiếp và thao tác xem lịch sử.
- Nút loa nằm cạnh câu Nhật trên dải, tách khỏi thanh công cụ bên phải.
  Nghe câu Nhật đang hiện, ngay cả khi chỉ hiện bản dịch; bấm lại
  để dừng. Chuột phải nghe phần Nhật đã chọn. Khi chưa chọn giọng, nút loa
  mở tab Giọng đọc để kết nối/chọn giọng/nghe thử.
- Tab Giọng đọc: danh sách từ API có tìm kiếm, biến thể/dialect, nghe mẫu,
  âm lượng riêng, công tắc tự đọc mặc định tắt; endpoint trong mục nâng cao.
  Voice ID nguyên vẹn dùng cho API, nhãn Akane/Aoi chỉ làm dễ đọc tên.
- Tự đọc tạm ngưng khi game mất focus hoặc xem lịch sử; quay lại chỉ nhận
  câu mới, không đọc hàng đợi câu đã bỏ qua. Nghe thủ công trong lịch sử
  vẫn được. Không tự phát hiện câu nào có lồng tiếng gốc.
- `~/.config/vntext/tts.json`, version 1: endpoint global, voice/auto/volume
  theo game. Ryujinx dùng Title ID trong tiêu đề X11; nếu chưa đọc được ID
  thì dùng khóa theo phiên PID và ghi rõ trong UI. Đổi Title ID trong cùng
  phiên sẽ dừng TTS, yêu cầu mở lại phụ đề để tránh dùng kịch bản/giọng cũ.
- Backend trả WAV hoàn chỉnh. Client giữ WAV trong memfd (file ẩn danh trong
  RAM), phát bằng pw-play qua `/proc/<reader>/fd/<n>`, không ghi WAV ra đĩa.
  Child chỉ được dừng theo handle/PID của mình; PDEATHSIG giúp player thoát
  khi reader chết. Cleanup khi thoát bình thường dừng player và đóng memfd.
- Cache client chỉ giữ một WAV gần nhất, tối đa 16 MiB, trong 30 giây;
  key gồm endpoint/voice/text. Đổi voice/endpoint xóa cache. Cache backend
  giữ nguyên. Log chỉ ghi thời gian, số byte và hit/miss, không ghi câu game.

### Kiểm chứng

- Unit tests có HTTP server giả và player process thật đọc memfd, không phát
  ra loa: Unicode/voice đúng, burst chỉ POST first+latest, bỏ response cũ,
  dịch không đọc lặp, replay không POST, dừng đúng PID, đổi voice/tắt auto,
  WAV hỏng, voice bị gỡ, backend offline/timeout, thiếu player, cấu hình và
  Title ID. Bộ unit đầy đủ đạt 129 test, 4 bài GUI/live bỏ qua mặc định.
- GUI cô lập KWin ở 1024×768 và 800×600: năm tab, gồm Giọng đọc, vẫn nằm
  trong màn hình và đóng được; cấu hình/key tạm, không mở hoặc điều khiển game.
- Test local thật dùng câu mẫu cố định và Aoi ns: nhận WAV 401.778 byte,
  request khoảng 6 ms, pw-play thoát thành công sau khoảng 4.254 ms gồm
  thời gian phát. Không dùng số 6 ms làm benchmark synth chưa cache.
- Lệnh live chạy riêng có guard:
  `VNTEXT_TTS_LIVE=local_fixture <test-binary> --exact tts::tests::live_sample --ignored --nocapture`.
  Log: `target/diagnostics/tts-live.log`; GUI: `target/diagnostics/tts-ui/`.

### Còn nghiệm thu

Chủ dự án chọn giọng/nghe câu đang xem/nghe phần chọn, bật tự đọc rồi chuyển
câu và dừng trong game thật. Kiểm tra nghe giọng có đúng, âm lượng và focus.
Không tuyên bố hủy synth trong DLL: stop/timeout chỉ ngăn playback/loại
pending của client; request native đang chạy có thể tiếp tục ở backend.
Backend chưa có scheduler hữu hạn/quota cache; giữ kế hoạch bước 3 bên dưới.
Điều chỉnh tốc độ giọng/streaming PCM/mapping nhân vật chưa nằm trong bản này.

## Tối ưu live TTS — đã triển khai ngày 2026-11-06

Chủ dự án duyệt thực hiện pipeline sau khi đối chiếu thực hành streaming,
warm-up và prefetch. Vẫn trên `feat/uiux-before-tts`, chưa commit/merge.

### Bằng chứng và kết quả

- Dịch vụ engine chạy lâu có gần 958 MiB swap: có thể ảnh hưởng cold voice;
  không kết luận mọi spike do swap. Worker nay đo load_ms, first_pcm_ms,
  synth_ms, native_ms; scheduler đo queue_ms. Client đo queue/first PCM/
  complete. Thời gian tới PipeWire chưa phải thời gian đo tại loa vật lý.
- Câu mẫu ngắn lần nạp giọng mới: 4.279 ms tổng HTTP, load 2.322 ms,
  PCM đầu 3.680 ms. Câu dài khi giọng đã nạp: tổng khoảng 4.839 ms,
  PCM đầu 528 ms. Callback audio thực sự có trước khi talk() hoàn thành.
- Client streaming thật với Aoi: nhận PCM đầu khoảng 391 ms, tổng stream
  1.165 ms cho một câu mẫu; phát hoàn tất bình thường. Lần cache hit sau:
  khoảng 14 ms từ enqueue tới PCM, nghe lại RAM 0 ms phần request; test
  xác nhận hai lần nghe liên tiếp dùng cùng PID PipeWire.
- Một câu dài khác khi voice cold cho thấy PCM đầu 3.889 ms/tổng 8.753 ms:
  streaming không loại bỏ được thời gian nạp voice. Đây là lý do có warm-up
  thật; các số đo dùng câu khác nhau, không quảng cáo là benchmark đối chứng.
- API Anki tương thích: WAV cold/hit hợp lệ, cache-stream PCM bằng WAV.
  Cache hit khi đang có preparation vẫn khoảng 4 ms, không chờ native lock.

### Những thay đổi hiện tại

1. Backend tối ưu có source tại `integrations/aivoice2/`, không sửa trực
   tiếp source/binary gốc trong aivoice2-re. Drop-in service riêng trỏ về
   backend/worker của nhánh thử. Hướng dẫn build/API/rollback trong README
   thư mục đó. Cùng prefix/port/data; không khởi động thêm engine song song.
2. `/v1/speak` giữ contract Anki, thêm timing. Cache-hit ngoài native lock.
   Một scheduler có foreground FIFO ưu tiên, giới hạn 24 việc chờ; mỗi
   session có một preparation pending mới nhất, tối đa 24 session.
   Cancel/revision chỉ ảnh hưởng pending của đúng session. Chưa có fairness
   cho background khi foreground liên tục có việc; running native không hủy.
3. `/v1/prepare mode=warm` gọi worker nạp/chọn Lang+Voice thật, không đọc
   WAV cache và không tổng hợp câu giả. Chọn/mở giọng khởi động warm ở nền;
   nếu đang có câu ngắn cần chuẩn bị thì tổng hợp nó cũng làm nóng giọng.
4. Chế độ **Chuẩn bị audio trước khi nghe**, mặc định bật, tự lưu theo game.
   Chuẩn bị câu hiện tại khi tự đọc tắt, giọng đã chọn và game có focus.
   Sau khi nghe, chuẩn bị một câu kế tiếp dưới 161 ký tự từ kịch bản.
   Dự đoán index+1 có thể sai ở nhánh truyện; không tự phát dự đoán, chỉ
   dùng audio có cùng text/voice. OCR không có câu kế tiếp. Menu lựa chọn
   bắt đầu bằng ▶ không được dự đoán. Không prefetch hàng loạt cả kịch bản.
5. Native callback gửi PCM frame qua stdout bên cạnh WAV cache; backend
   `/v1/speak/stream` chuyển PCM16 mono 48 kHz ngay, frame cuối xác nhận
   thành công. Client bỏ stream hỏng/thiếu frame cuối, không cache audio lỗi.
   Có thể đã nghe một phần trước khi phát hiện lỗi ở cuối stream.
6. `src/tts_audio.rs` giữ pw-play raw PCM stdin sau khi một câu phát hết,
   bộ đệm tối đa 16 MiB. Dừng/chuyển câu khi bận đóng đúng player, không
   để PCM cũ còn trong thiết bị; lần sau tạo lại. PDEATHSIG giữ cleanup
   khi reader chết. Cache client giới hạn 32 MiB/120 giây, thay cho một câu/30
   giây. Số bản ghi thực tế tối đa 7; ghi chú cũ nêu mức dự kiến 8.
   Ngày 2026-11-07 bổ sung quota WAV backend 512 MiB và hết hạn 7 ngày.
7. Capability negotiation: backend cũ tự dùng lại WAV/memfd. Session random
   riêng theo reader; foreground và preparation dùng cùng native worker.
   HTTP local tắt proxy, Connection: close để tương thích server HTTP/1.0.

### Kiểm chứng và giới hạn

Đợt cache 2026-11-07: chủ dự án duyệt quota WAV 512 MiB, loại file ít dùng
gần đây nhất và hết hạn 7 ngày. `integrations/aivoice2/wav_cache.py` giữ
lease cho file đang tổng hợp/trả HTTP, dọn khi khởi động/sau publish/mỗi
60 giây và dọn lại khi lease cuối đóng nếu còn vượt quota. Chỉ tác động
WAV cache có tên hash của backend, không theo symlink. Healthz có thống kê
cache. Giữ RAM 32 MiB/120 giây và lookahead 1 đoạn; không triển khai 3–5
đoạn khi chưa có dữ liệu nhánh/tỷ lệ sử dụng dự đoán.

13 test Python qua: quota, LRU sau restart, hết hạn, file ngoài phạm vi,
lease lồng nhau và dọn đồng thời, publish atomic; HTTP WAV và cached PCM
vẫn trả đủ dữ liệu khi quota chỉ 1 byte, rồi file mới bị dọn sau giao xong.

- Rust: 131 test đạt, 4 GUI/live test bỏ qua mặc định. Thêm test giới hạn
  frame, stream bị cắt, stale response, final error, preparation và output WAV.
- Python backend: 7 test đạt cho bounded queue, ưu tiên foreground, thay/hủy
  preparation đúng session, cache-hit không chờ lock, warm bỏ qua cache,
  drain PCM frame trước final native status.
  Revision mới cũng loại foreground cũ còn chờ của đúng session, không ảnh
  hưởng job Anki không mang session; native đang chạy vẫn không bị hủy.
- GUI năm tab đạt ở 1024×768 và 800×600 trong KWin riêng; có công tắc
  preparation, không giả lập đầu vào hoặc điều khiển game trên desktop.
- Live fixture phát Aoi qua streaming và replay cùng player đạt; compatibility
  WAV/PCM thật đạt. File đo và log: `target/diagnostics/latency/`.
- Cần chủ dự án nghe trong game thật: cold/warm voice, bấm nghe sau khi
  chờ preparation, tự đọc khi chuyển câu và dừng nhanh. Không gọi Gemini
  hoặc nguồn dịch trả phí trong kiểm thử. Không cam kết hủy synth native,
  latency cố định, voice luôn giữ trong RAM khi hệ điều hành có áp lực nhớ.

## Bước 0 — Mở được vntext

Đã tái hiện lỗi bản đang được menu ứng dụng gọi:

- Binary `target/release/vntext` thoát ngay, mã thoát 0, với lỗi
  `cannot reach the X server: Connection refused (os error 111)`.
- KWin/XWayland vẫn chạy, `DISPLAY=:0`. File `/tmp/.X11-unix/X0` không
  còn tồn tại, nhưng socket abstract `@/tmp/.X11-unix/X0` vẫn nhận kết nối.
  Chưa xác định tiến trình nào đã xóa file; không quy nguyên nhân cho Wine.
- Transport Rust của `x11rb 0.14.0` thử socket filesystem rồi TCP local,
  không thử socket abstract. Launcher cũ yêu cầu X11 trước khi tạo cửa sổ;
  desktop entry dùng `Terminal=false` nên người dùng không thấy thông báo.

Sửa `screen.rs`: nếu kết nối thường thất bại, thử socket abstract của đúng
display local. Dùng Xauthority như bình thường, giữ screen đã yêu cầu;
không đổi remote display thành local hoặc tắt xác thực. Cách này không tạo
lại socket bị mất cho các ứng dụng khác trong desktop.

Sửa `launcher.rs`: thiếu X11 vẫn dựng giao diện và hiển thị chẩn đoán, thử
lại khi làm mới danh sách. GTK vẫn phải kết nối được Wayland hoặc X11 để
hiện cửa sổ. README và lời hướng dẫn trong `doctor` đã cập nhật.

Kiểm chứng:

- `cargo test --release --locked`: 119 đạt, 1 bỏ qua (test Gemini thật).
- `cargo build --release --locked`: đạt; binary menu ứng dụng gọi đã cập nhật.
- `doctor` dùng XWayland thật: đạt kiểm tra X11 dù file socket còn thiếu.
- Xvfb có Xauthority riêng, D-Bus riêng: kết nối bình thường đạt; xóa
  **chỉ socket của Xvfb do bài kiểm tra tạo** vẫn đạt; cookie sai bị từ chối.
  Cửa sổ launcher thực sự được map sau khi xóa socket.
- Wayland thật với `DISPLAY` trỏ display không tồn tại, D-Bus riêng:
  launcher vẫn chạy và trả lời lệnh `Show` sau khi ghi nhận lỗi X11.
- Không mở game, giả lập thao tác đầu vào hoặc gọi API dịch trả phí.

Binary cũ có bản sao theo SHA-256 trong `target/startup-backups/`.
Log và script thử nghiệm ở `target/diagnostics/`, được Git bỏ qua.
Test tay còn lại: mở từ menu, chọn game đang chạy, kiểm tra dải nổi;
không coi kiểm tra launcher là nghiệm thu đọc game hoặc TTS.

### Sửa tiếp sau khi thử chọn game

- Log launcher báo `cannot serve on port 8777: Address already in use`.
  Cổng này thuộc một ứng dụng Python khác; lỗi xảy ra trước khi dựng overlay
  khi bật trang đọc web. Không dừng hoặc đổi cấu hình ứng dụng đó.
  Hub nay bind cổng local trống nếu cổng yêu cầu đang bị chiếm, dùng cổng
  thực tế để kiểm tra Host và mở trình duyệt.
- Higurashi trên Ryujinx đang chạy vẫn nhận được
  `shin (Higurashi Hou layout), 190588 passages` khi kiểm tra lại bằng
  `auto <pid>` không dịch, không web, không overlay, bỏ toàn bộ stdout.
  Không thiếu profile. Launcher cũ chỉ nhận diện mỗi PID một lần và giữ
  `Unknown` đến khi cửa sổ game biến mất; không tự kiểm tra lại khi kịch
  bản nạp sau đó. Chưa quan sát thời điểm lần nhận diện đầu thất bại.
- Thêm nút **Nhận diện lại**, tự thử lại game đang chọn chưa nhận ra sau
  mỗi 30 giây khi launcher đang hiện. Không quét chồng cùng PID. Sửa nhãn
  để phân biệt chưa nhận ra ở thời điểm kiểm tra với thiếu hỗ trợ vĩnh viễn.
  Giữ nguyên code engine; sau khi đổi game trong emulator có thể dùng nút
  nhận diện lại. Dải nổi vẫn chỉ hiện khi game có focus.
- Test HTTP thật với cổng bị chiếm: phục vụ được trên cổng mới, Host của
  cổng cũ bị từ chối, listener chiếm cổng ban đầu vẫn nguyên vẹn.
- Kiểm chứng lần sửa này: 120 test đạt, 1 test Gemini thật bỏ qua; build
  release và `git diff --check` đạt. Đã mở lại launcher bản mới, log của
  chính launcher xác nhận `shin (Higurashi Hou layout)` cho Ryujinx đang
  chạy. Chờ chủ dự án chọn game và kiểm tra dải nổi; không tự bấm hoặc
  khởi chạy OCR/dịch bằng key đã lưu.

## Bước 1 — Client TTS trong tiến trình đọc

Tái sử dụng backend A.I.VOICE2 độc lập với Anki, không load DLL hoặc khởi
tạo Wine prefix từ vntext. Hợp đồng hiện tại:

| Endpoint | Ý nghĩa |
|---|---|
| `GET /healthz` | Trạng thái `ready`; bản mới có `instance_id` |
| `GET /v1/voices` | `voices` và `default_voice`; dùng ID thực tế từ API |
| `POST /v1/speak` | JSON `{text, voice}` UTF-8, trả WAV; lỗi JSON với HTTP 400/503 |

Endpoint mặc định `http://127.0.0.1:17862`. Đã kiểm tra read-only backend
trên máy; việc kết nối thành công chưa phải test tổng hợp/phát âm thanh.
Không sửa `/v1/speak` hoặc cơ chế chèn media Anki trong bước client này.

Điểm nối đã khảo sát trong source:

- `main.rs::follow()` phát `type=message`, `seq`, `source=profile`, `name`,
  `lines` sau khi xác định đoạn mới. Đọc `lines`, không đọc `raw`.
- `ocr.rs` phát cùng schema sau khi có kết quả OCR, `source=ocr`.
- `web::Hub::subscribe()` chỉ nhận sự kiện mới, không replay lịch sử.
  Subscribe trước khi extractor bắt đầu phát; mỗi tiến trình đọc có một
  worker. Không chạy worker thứ hai trong launcher hoặc overlay.
- Chỉ đọc `message`; bỏ qua `translation`, `notice` và thao tác xem lại lịch
  sử. Câu Nhật lấy từ `lines.join("\n")`; tên nhân vật để riêng.

Luồng dự kiến: event mới → chuẩn hóa tối thiểu → một ô câu chờ mới nhất →
HTTP worker → kiểm tra generation → phát WAV tại client. Không chặn vòng
đọc game hoặc GTK bằng HTTP/synth. Không để receiver tích hàng đợi trong
lúc worker đợi HTTP: bộ nhận event phải tiếp tục thay câu chờ và tăng
generation. Chỉ có một request đang chạy của client tại một thời điểm.

Câu mới, tắt TTS, đổi voice hoặc đổi session phải dừng player do client sở
hữu và làm kết quả cũ hết hiệu lực. Chỉ dừng PID player của mình. Không
phát kết quả trả muộn; timeout/ngắt HTTP chưa chứng minh native synth đã
dừng. Lỗi TTS không được dừng extraction hoặc translation.

Mặc định tắt tự đọc để tránh chồng lên lồng tiếng của game. Lưu bật/tắt và
voice theo game, bổ sung endpoint và thông báo lỗi dễ hiểu. Giọng phải lấy
từ API, không ghi cứng danh sách. `pw-play` có sẵn trên máy; cần kiểm tra
WAV, giới hạn dung lượng và dọn file tạm trước khi chọn làm player chính.

## Bước 2 — Kiểm thử trước khi thử game

Dùng HTTP giả và câu Nhật tự viết: Unicode, text trùng, burst câu mới,
kết quả cũ trả muộn, đổi voice, tắt TTS, backend tắt/timeout, WAV hỏng, thiếu
player. Đo số request và câu thực sự được phát; không chỉ kiểm tra HTTP 200.
Sau đó mới thử một câu tổng hợp từ backend thật và nghiệm thu nghe với game.
Mapping nhân vật → giọng để giai đoạn sau.

## Bước 3 — Scheduler backend khi cần mở rộng

Backend hiện xử lý synth tuần tự dưới lock, chưa có hàng đợi hữu hạn hoặc
hủy job theo session. Một request vntext bị timeout có thể tiếp tục chạy ở
server; client bỏ kết quả không giải quyết toàn bộ giới hạn này.

Nếu cần đảm bảo tải nhiều client, bổ sung scheduler chung: hàng đợi hữu hạn,
session/request ID, deadline và hủy pending. vntext chỉ thay/hủy câu của
session mình; không hủy tác vụ Anki. Running synth chỉ được hủy thật sau
khi xác minh API native. Giữ `/v1/speak` tương thích.

Định hướng backend chi tiết có trong repo bàn giao A.I.VOICE2 tại
`docs/TTS_CLIENTS_ROADMAP_VI.md`. Không tuyên bố hỗ trợ nhiều client ổn định
hoặc hủy native synthesis khi mới hoàn thành client vntext.
