# Ghi chú kỹ thuật từng engine

Tài liệu này ghi lại, cho từng profile trong `src/engines/`, script của game nằm ở đâu, được mã hoá thế nào, và tool biết game đang đứng ở câu nào bằng cách gì. Đọc nó khi sửa một profile có sẵn hoặc khi cần ví dụ để viết profile mới (xem mục "Mở rộng engine khi gặp game mới" trong [README](../README.md)).

Mỗi họ engine có một profile (`engines/`) biết script của game nằm ở đâu và engine giữ vị trí đang đọc ở ô nhớ nào. `auto` thử lần lượt các profile và dùng cái nhận ra game.

## shin: Entergram (Umineko, Higurashi)


1. Tìm file kịch bản trong RAM qua chữ ký `SNR `, đọc cả file.
2. Tách mọi lệnh thoại `MSGSET` (opcode `0x86`), kiểm tra bằng việc số thứ tự thoại phải liên tiếp. Bố cục lệnh lệch nhau giữa các game: bảng `LAYOUTS` liệt kê các kiểu đã biết (Umineko 7 byte đầu, Higurashi 8), tool thử hết và giữ kiểu đọc ra nhiều đoạn hơn.
3. Tìm ô nhớ vị trí của máy ảo script: một ô 32-bit chứa vị trí bắt đầu của một lệnh thoại, và 8 byte trước nó là vị trí ngay sau lệnh đó. Chỉ tìm trong vùng nhớ chứa kịch bản, tức RAM của game; bộ nhớ riêng của emulator cũng có dữ liệu khớp dạng này.
4. Đọc các ô tìm được mỗi 50 ms; khi ô trỏ sang lệnh thoại khác thì in đoạn mới.
5. Tìm lại vị trí ở luồng nền khi 3 giây không có đoạn mới (giãn dần tới 30 giây), và tìm liên tục khi chưa có gì để bám. Ô vị trí đổi chỗ khi load save hoặc về màn hình chính.

Không phải ô nào khớp dạng cũng là ô thật. Game giữ các bản sao trạng thái máy ảo (trạng thái của file save vừa load, bộ đệm đọc file save) trỏ tới một đoạn chẳng liên quan. Cách phân biệt:

- Ô nằm ở đầu trang nhớ, tại offset `0x10`, được tin ngay. Mọi ô thật quan sát được trên cả hai game đều nằm ở đó, bản sao thì không. Nhờ vậy lần tìm thông thường chỉ đọc vài byte đầu mỗi trang (dưới 1 giây thay vì khoảng 5 giây quét cả RAM).
- Ô ở chỗ khác chỉ được tin khi nó tiến từng bước nhỏ về phía trước hai lần liên tiếp, như một người đang đọc. Một phần mười số lần tìm là quét kỹ để thấy các ô loại này.

Text của shin là Shift-JIS, trong đó hiragana và vài dấu câu được ghi bằng katakana nửa độ rộng một byte, xen với lệnh `@` (`@r` xuống dòng, `@k` chờ bấm, `@v` voice, `@c` màu, `@b…@<…@>` furigana, ...). `engines/shin/text.rs` chuyển về text đọc được. Bảng ký tự và cú pháp lệnh lấy từ [DCNick3/shin](https://github.com/DCNick3/shin) (MPL-2.0).

## bishop: BISHOP (BSXScript)

Profile bishop (`engines/bishop/`), cho các game dùng `bsxx.dat`:

1. Tìm chữ ký `BSXScript 3.` trong RAM, rồi tìm ô nhớ trỏ tới nó; ô kế tiếp là kích thước script. Cặp ô đó nằm trong khối trạng thái của máy ảo, ở vị trí cố định suốt lần chạy.
2. Đọc script: bảng tên nhân vật, bảng câu thoại (UTF-16), và duyệt mã lệnh để lấy mọi lệnh "hiện câu thoại" (`0x1D`) theo thứ tự.
3. Trong khối trạng thái, ô vị trí là ô chứa điểm đầu của một lệnh hiện thoại, khi một ô gần đó chứa điểm cuối của chính lệnh ấy.
4. Menu lựa chọn: mỗi lựa chọn là một lệnh `0x33` mang số thứ tự câu làm nhãn, xen giữa là các lệnh khác, và lệnh `0x34` chờ người đọc chọn. Khi menu đang hiện, ô vị trí vẫn giữ câu thoại trước đó, còn bộ đếm chương trình trong khối trạng thái đứng ngay sau lệnh `0x34`; tool nhận ra menu qua giá trị đó và hiện nó thành một đoạn riêng, mỗi lựa chọn một dòng có dấu `▶`. Chế độ xem lại cảnh cũng dùng các lệnh này. Đã kiểm tra trên 牝教師, 牝教師4 và 隷従の制服 (BSXScript 3.0, 3.1, 3.3); các game còn lại dùng cùng lệnh nhưng chưa thử.
5. Giải mã text: `\n` là xuống dòng trong khung thoại (bỏ), `\b` và `\s` đổi cỡ chữ (bỏ), `\A` và `\B` là họ và tên nhân vật chính (đọc từ `bsxx.ini` trong thư mục game), `U+E000` là dấu ♥. Các ký tự `U+E0xx` khác được suy đoán là kana có dấu đục; chưa gặp trong game để đối chiếu.

Định dạng file và độ dài lệnh lấy từ [crskycode/BSXScript_Tool](https://github.com/crskycode/BSXScript_Tool).

## spt: BLACKRAINBOW đời cũ (script .spt)

Profile spt (`engines/spt/`), cho 催眠術2 và 催眠術2 URA:

1. Engine này không giữ file script trong RAM (nó biến mỗi script thành danh sách liên kết các lệnh), nên gói script `*_spt.dat` được đọc từ thư mục game.
2. Gói gồm nhiều file `.spt`; mỗi file là một dãy lệnh, lệnh `0x08` là câu thoại. Text là Shift-JIS, cứ 4 byte thì byte đầu bị đảo bit thấp nhất. Tên người nói nằm đầu câu dạng `【tên】`.
3. Vị trí đang đọc: chương trình giữ ở chỗ cố định phần đầu của script đang chạy (tên, số lệnh, kích thước), sau đó 12 byte là số thứ tự lệnh hiện tại, rồi tên script lặp lại một lần nữa. Máy ảo đứng yên ở lệnh thoại suốt lúc câu đó hiện trên màn hình; điều này đã được đo trên cả hai game.
4. Menu lựa chọn là lệnh `0x0e`: text Shift-JIS thường (không đảo bit), mỗi lựa chọn kết thúc bằng hai ký tự `\0` (dấu gạch ngược và số 0, không phải byte NUL). Khi menu hiện, số thứ tự lệnh đứng đúng ở lệnh menu, nên tool chỉ cần coi nó như một câu thoại và hiện thành một đoạn riêng, mỗi lựa chọn một dòng có dấu `▶`. Chỉ 催眠術2 có menu (36 cái trong 31 script); 催眠術2 URA không có. Đã kiểm tra ở một menu của 催眠術2.

Định dạng lấy từ một bộ công cụ patch tiếng Anh cho hai game này (`blackrb_spt.py`), không rõ tác giả.

Khi một profile cần file của game (gói script của spt, `bsxx.ini` của bishop), thư mục game được xác định bằng nơi chứa file exe đang được nạp vào process, rồi mới tới thư mục làm việc. Cách đầu không phụ thuộc launcher: dưới Wine, file exe hiện ra với đường dẫn thật trên Linux.

## med: MED (催眠術3, 4, RE, WIZARDLINKS)

Profile med (`engines/med/`), cho 催眠術3, 催眠術4, 催眠術 RE và WIZARDLINKS:

1. Gói script `md_scr.med` được đọc từ thư mục game, rồi tới các gói vá `md_scr2.med`... nếu có; script trong gói sau thay script trùng tên trong gói trước, như game làm. Mỗi file trong gói bị mã hoá bằng cách cộng từng byte với một khoá lặp lại; khoá được dò ra từ file `_VIEW`, vì 24 byte đầu của nó giống nhau ở mọi game dùng engine này.
2. Mỗi script là một dãy lệnh (số dòng nguồn, độ dài, mã lệnh, tham số) rồi tới bảng chuỗi; lệnh mang số thứ tự của chuỗi. Một khung thoại là một chuỗi lệnh: có thể có tên người nói (`13`), rồi text (`1`) và xuống dòng (`0`), kết thúc bằng lệnh xoá khung (`4`).
3. Vị trí đang đọc lấy từ trạng thái máy ảo trong RAM, gồm (tính theo ô 32-bit): độ dài phần mã lệnh của script đang chạy, cách 4 ô là bộ đếm chương trình, cách 4 ô nữa là bản sao của bộ đếm, và ngay sau đó là con trỏ tới tên script. Tool tìm cụm này bằng cách quét bộ nhớ, rồi mỗi lần đọc lại tên script và bộ đếm.
4. Bộ đếm trỏ tới lệnh kế tiếp; trong lúc một khung thoại hiện trên màn hình, máy ảo đứng ở lệnh xoá khung của nó.
5. Lựa chọn: lệnh `33` mang danh sách vị trí của các lệnh `10`, mỗi lệnh `10` giữ chữ của một phương án. Tool gom chúng thành một khung riêng, người nói ghi là `選択肢`, mỗi phương án một dòng. Trong lúc chờ chọn, máy ảo đứng ở lệnh `33` hoặc lệnh ngay sau nó.

Phải nhận diện script bằng tên: rất nhiều script trong cùng một game có phần mã lệnh mở đầu giống hệt nhau (ở 催眠術4, 339 script chỉ có 64 kiểu mở đầu khác nhau), nên so mã lệnh sẽ nhận nhầm.

Định dạng gói lấy từ GARbro; cách dò khoá và định dạng script lấy từ một bộ công cụ dịch cho engine này (`MED.py`, `SCE.py`), không rõ tác giả.

## ark: ArkEngine của Astronauts (Unity, Mono)

Profile ark (`engines/ark/`), cho loạt 極限痴漢特異点 (Unity kiểu Mono):

1. Script là file văn bản, đóng gói và mã hoá trong `<game>_Data/StreamingAssets/scripts.bin` kèm `scripts.idx`. Mọi thứ trong gói là AES-256-CBC với khoá và vector khởi tạo cố định, chung cho cả sáu bản; mỗi mục của index và mỗi script được bọc thêm một lớp MessagePack.
2. Trong script, một khung thoại nằm giữa `\text` (hoặc `\text("người nói",…)`) và `\endtext`.
3. Trong RAM, đối tượng `ScriptParser` của game giữ tên script đang chạy, câu đang hiện và số dòng hiện tại. Tool tìm đối tượng này bằng năm trường liền nhau trỏ tới các chuỗi `"\n"`, `"\r"`, `" "`, `"["`, `"/"` (các dấu tách mà nó dùng để cắt dòng script).
4. Khung đang hiện được xác định theo nội dung chứ không theo vị trí trường, vì bố cục đối tượng khác nhau giữa các bản: một trường chuỗi là tên script, một trường khác là text trùng với một khung của script đó (so sau khi bỏ khoảng trắng). Nếu script có nhiều khung cùng nội dung, khung có số dòng xuất hiện trong đối tượng được chọn.

5. Lựa chọn là một dãy `\MENU(số,"chữ",…)` kết thúc bằng `\MENUEND`. Menu không để lại text riêng trong đối tượng: text vẫn là của khung thoại đứng trước nó, còn bộ đếm dòng đã chuyển vào phạm vi menu. Nên khi số dòng của khung thoại không còn trong đối tượng mà có một số nằm trong phạm vi của menu ngay sau khung đó, tool coi là menu đang hiện.

Toàn bộ thông tin lấy từ chính mã của game, dịch ngược từ `Assembly-CSharp.dll` bằng ILSpy.

## yuris: YU-RIS

Profile yuris (`engines/yuris/`), cho các game YU-RIS (聖奴隷学園 1–3, Saimin Seishidou):

1. Script đã biên dịch (`ystNNNNN.ybn`) nằm trong các gói `.ypf` của game. Một game có nhiều gói: gói gốc và các gói `update…`; script trong gói update thay script cùng tên trong gói trước.
2. Mỗi script gồm bốn phần (lệnh, tham số, dữ liệu, số dòng nguồn), mỗi phần XOR với cùng một khoá 4 byte riêng của game. Ba byte cao của khoá lộ ra qua số dòng nguồn đầu tiên; byte thấp là giá trị duy nhất làm cho, trong mọi script, số dòng không giảm và mọi mã lệnh đều tồn tại.
3. Text là các lệnh `WORD` nối tiếp, kết thúc bằng lệnh `RETURNCODE`, là chỗ script chờ người đọc. Tên người nói nằm đầu dòng dạng `【tên】`.
4. Engine giữ script đang nạp ở dạng đã giải mã trong RAM. Với mỗi tác vụ chạy script, nó có một khối ngữ cảnh chứa số thứ tự lệnh kế tiếp, con trỏ tới phần dữ liệu và phần số dòng của script, và số hiệu script. Số thứ tự lệnh nằm trước một dãy con trỏ dài ba ô ở phiên bản engine cũ (479) và bốn ô ở phiên bản mới (494); tool thử cả hai chỗ và lấy chỗ chứa một số hợp lệ, vì chỗ còn lại là con trỏ. Tool tìm các script đang nạp qua chữ ký `YSTB`, rồi tìm cặp ô liền nhau trỏ tới hai phần đó của cùng một script. Chỉ ngữ cảnh của script có thoại được theo dõi; các script nội bộ của engine cũng chạy thành tác vụ nhưng không có text.

5. Menu lựa chọn không phải lệnh của engine mà là thủ tục `ES.SEL.SET` trong thư viện script, gọi bằng `GOSUB` với nhãn từng lựa chọn làm tham số. Khi menu đang hiện, tác vụ chạy bên trong thư viện (một script không có thoại), nên tool nhìn vào chồng lệnh gọi của ngữ cảnh: một số nhỏ rồi các con trỏ tới khung `?, lệnh sẽ quay về, số hiệu script`, mỗi tầng một khung. Khi tác vụ đang ở trong thư viện thì khung đầu tiên luôn là lệnh gọi từ script truyện đã đưa nó vào đó, nên tool chỉ cần xem khung này: nếu nó quay về ngay sau một lệnh gọi menu thì menu đó đang hiện. Chồng lệnh gọi nằm cách ô ngữ cảnh 0x110 byte ở đời 488 và 0x270 byte ở đời 479, nên tool dò trong cả vùng chứ không ghi cứng. Đã kiểm tra trên Saimin Seishidou (488) và 聖奴隷学園 (479); 聖奴隷学園3 (494) chỉ có một menu nên chưa thử, 聖奴隷学園2 không có menu nào. Kiểu chọn bằng bản đồ (`ES.CSEL.SET`) không có chữ nên bỏ qua.

Định dạng file lấy từ [shimamura-sakura/yuri](https://github.com/shimamura-sakura/yuri).

## age: AGE của Eushully (SYS4)

Profile age (`engines/age/`), cho engine AGE của Eushully với script `SYS4` (封緘のグラセスタ):

1. Script là file `.BIN` đã biên dịch. Phần lớn nằm trong các gói `.ALF`, liệt kê trong `SYS4INI.BIN` (game gốc) và `APPENDnn.AAI` (các đĩa append); một số nằm lẻ trong thư mục game, là bản vá. Danh sách file trong các file chỉ mục này nén LZSS.
2. Mỗi script là một dãy lệnh, mỗi lệnh là mã lệnh và các tham số (kiểu, giá trị). Chuỗi nằm sau phần lệnh, là Shift-JIS đảo từng bit, kết thúc bằng `FF`. Một khung thoại là các lệnh `show-text` (và `display-furigana`, chỉ giữ chữ gốc, bỏ phần phiên âm) cho tới lệnh `wait-for-input`.
3. Tên người nói không nằm cạnh câu thoại: script gán số hiệu người nói vào một biến toàn cục rồi gọi thủ tục vẽ tên, thủ tục này tra tên trong các chuỗi toàn cục mà script khởi tạo đã đặt sẵn. Tool tự tìm thủ tục đó để biết biến nào và bảng nào, nên các số này không ghi cứng trong code. Lời dẫn gọi một thủ tục gán -1 vào biến, tức là không có tên.
4. Các mã `F040`–`F043` là glyph riêng của engine (gạch ngang và thanh kéo dài nguyên âm), được đổi thành `―` và `ー`.
5. Trong RAM, với mỗi script đang chạy, trình thông dịch giữ con trỏ tới đầu phần lệnh, con trỏ tới lệnh đang đứng và bản sao 12 số trong header của script (số biến cục bộ, độ dài và vị trí ba bảng sau phần lệnh). Tool tìm bản ghi này qua 12 số đó, nhận ra script nào đang chạy, và lấy khung thoại chứa lệnh đang đứng. Nếu hai script có cùng 12 số (bản gốc và bản vá), tool so phần lệnh trong RAM để chọn.

6. Menu lựa chọn không có lệnh riêng: script gán nhãn từng lựa chọn vào một mảng chuỗi (`set-string` vào phần tử mảng), câu hỏi nếu có thì gán vào một chuỗi toàn cục ngay trước đó, rồi gọi một script hệ thống (`call-script`) để vẽ menu và chờ. Tool tìm các `call-script` có từ hai nhãn trở lên được gán trong đoạn code thẳng ngay phía trước. Khi menu hiện, bản ghi của trình thông dịch là của script hệ thống, còn bản ghi của script truyện bị đẩy xuống ô kế tiếp (cách 0x168 byte), đứng đúng ở lệnh `call-script`; tool đọc ô đang chạy rồi nhìn các ô sau nó. Bản ghi cũ sót lại sau khi menu đóng bị loại bằng cách so con trỏ code: nếu trùng với script đang chạy thì nó là bản cũ. Hầu hết menu là hộp có/không (khoảng 564 menu trong 124 script cảnh, chỉ 52 nhãn khác nhau). Đã kiểm tra trên một menu.

Định dạng file và bảng lệnh lấy từ [Kelebek1/Eushully-Decompiler](https://github.com/Kelebek1/Eushully-Decompiler).

## escude: Escu:de

Profile escude (`engines/escude/`), cho engine của Escu:de (廃村少女 弐):

1. Mọi thứ nằm trong các gói `ESC-ARC`: bảng file mã hoá bằng một khoá chạy sinh từ số gốc trong header, từng file nén LZW (`acp`). `script.bin` chứa script, `data.bin` chứa database.
2. Mỗi script có hai file: `.bin` là mã lệnh (mỗi lệnh một byte kèm 0–2 tham số 4 byte), `.001` là các câu thoại, XOR từng byte với `0x55`. Một trang thoại là `NAME` (nếu có người nói), một hoặc nhiều `TEXT`, rồi `PAGE`. Các lệnh `OPTION` sau trang là lựa chọn; tool hiện chúng thành dòng có dấu `▶`.
3. `NAME` mang số hiệu người nói, tra trong sheet `登場人物` của `db_scripts.bin`. Sheet `スクリプト` cho biết script số mấy là file nào.
4. Trong text, `<r>` là xuống dòng, `<ruby text='cách đọc'>chữ</ruby>` là furigana (giữ chữ, bỏ cách đọc).
5. Trong RAM, trình thông dịch có một bản ghi `SCR_ENV`: số hiệu script, vị trí đầu và cuối của lệnh vừa chạy, chính lệnh đó và các tham số của nó. Tool nhận ra bản ghi bằng cách đối chiếu với script: lệnh và tham số phải đúng là thứ nằm ở vị trí đó trong file, và lệnh đó phải thuộc một trang thoại. Bản ghi còn có số dòng nguồn, nhưng không dùng để đối chiếu được vì sau khi load save nó không được khôi phục. Trang đang hiện là trang chứa lệnh vừa chạy. Game giữ thêm một bản sao của bản ghi này làm trạng thái sẽ lưu vào save; ngay sau khi load hai bản giống hệt nhau, sau đó bản sao tụt lại. Vì vậy khi mọi bản ghi tìm được cùng chỉ một trang thì tool tin ngay, còn khi chúng khác nhau thì chờ xem bản nào tiến. Bản ghi này là cấu trúc đóng gói sát (không căn lề) và nằm ở địa chỉ lẻ, nên tool phải dò từng byte chứ không dò theo bước 4 byte như các engine khác.

Game đóng gói luôn mã nguồn engine của nó trong `haison2.bin`; ý nghĩa các lệnh và cấu trúc `SCR_ENV` lấy từ đó. Định dạng gói, database và script lấy từ [Chenx221/EscudeTools](https://github.com/Chenx221/EscudeTools).

## liar: Liar-soft (gsc)

Profile liar (`engines/liar/`), cho engine của Liar-soft (信天翁航海録):

1. Script là các file `.gsc` trong gói `scr.xfl`. Gói không nén: một bảng tên file kèm vị trí và độ dài.
2. Mỗi script gồm header chín số, phần lệnh, bảng vị trí chuỗi và các chuỗi Shift-JIS. Lệnh là mã 16 bit và các tham số có độ dài cố định theo mã. Bảng độ dài khác nhau giữa các đời engine; game này dùng bảng `RScript19` của LiarsoftTool.
3. Câu thoại là lệnh 81, tham số thứ sáu là số thứ tự chuỗi. Tên người nói nằm đầu chuỗi dạng `【tên】`; `^n` là xuống dòng; `|chữ[cách đọc]` hoặc `chữ[cách đọc]` là furigana, tool giữ chữ và bỏ cách đọc. Menu lựa chọn là lệnh 14, mang số lượng và số thứ tự chuỗi của từng lựa chọn; tool hiện nó thành một đoạn riêng, mỗi lựa chọn một dòng có dấu `▶`.
4. Trong RAM, trình thông dịch có một bản ghi cho script đang chạy: con trỏ tới phần lệnh, ba độ dài lấy từ header (phần lệnh, số chuỗi, phần chuỗi) và con trỏ tới lệnh sắp chạy. Ba độ dài đó khác nhau ở mọi script của game nên tool dùng chúng để tìm bản ghi và biết script nào đang chạy. Khi một câu đang hiện, con trỏ lệnh đứng ngay sau lệnh của câu đó.

Định dạng file và bảng lệnh lấy từ [Love-in-idleness/LiarsoftTool](https://github.com/Love-in-idleness/LiarsoftTool).

## ninetail: Unity của Ninetail (VenusBlood)

Profile ninetail (`engines/ninetail/`), cho các game Unity của Ninetail (VenusBlood Savior, Valkyrie):

1. Mỗi cảnh là một bundle `UnityFS` riêng trong `Standalone/scenario`, khối dữ liệu nén LZMA. Trong bundle có một text asset là JSON bắt đầu bằng `{"fileName":`; tool tìm nó theo nội dung thay vì đọc bảng đối tượng của Unity, nên không phụ thuộc phiên bản Unity.
2. Cảnh là một danh sách cut. Cut có thoại khi `window_enable` bật và `window.texts[0]` có chữ; dấu `;` trong text là xuống dòng. Tên người nói là `window.name`, nếu trống thì tra `window.tag` trong phần `@char` của bảng `advdb_dataset_jp` (text asset trong `resources.assets`). Cut có lựa chọn liệt kê chúng trong `select`; tool hiện mỗi lựa chọn thành một dòng có dấu `▶` sau câu thoại.
3. Trong RAM, đối tượng `AdvDataControlUser` giữ tên cảnh đang chạy (`filename`), số thứ tự cut đang hiện (`indexNum`) và số cut của cảnh (`indexMax`). Tool tìm nó bằng một ô trỏ tới chuỗi là tên một cảnh, theo sau là hai chuỗi nữa và, cách vài ô, một số nhỏ hơn số cut của cảnh đó đứng ngay trước chính số cut đó.
4. Khi nạp save, game tạo đối tượng mới và đối tượng cũ còn nằm lại trong RAM. Nếu chỉ tìm thấy một đối tượng thì tool tin ngay; nếu có nhiều thì tool chờ xem cái nào tiến theo người đọc.

Toàn bộ thông tin lấy từ chính mã của game, dịch ngược từ `Assembly-CSharp.dll` bằng ILSpy.


## Đọc bộ nhớ (`mem.rs`)

- Liệt kê vùng nhớ từ `/proc/<pid>/maps`, đọc qua `/proc/<pid>/mem`.
- Chỉ đọc trang đang có mặt trong RAM (xem qua `/proc/<pid>/pagemap`). Đọc một trang trống của vùng shm sẽ khiến kernel cấp phát nó, có thể ngốn hàng GB.
- Khử trùng lặp theo (file, vị trí trang). Ryujinx map cùng một file shm hàng nghìn lần; không khử thì quét 512 GB địa chỉ ảo thay vì khoảng 4 GB thật.
- Bỏ qua vùng thiết bị (`/dev/*` trừ shm) và vài vùng vô ích đã biết.
- Tốc độ quét khoảng 1 GB/giây trên một luồng.
