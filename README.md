[Docker Hub](https://hub.docker.com/r/bibica/fast-socks5-server-silent)

# Fast Socks5 Proxy Silent

<p align="center">
  <img src="https://raw.githubusercontent.com/bibicadotnet/fast-socks5-server-silent/refs/heads/master/_371d314a-4dd4-4797-86c1-71a1ce46f6b4_upscayl_2x_upscayl-standard-4x.png"/>
</p>

Fast SOCKS5 TCP and UDP client/server library for Rust async (Tokio)

Fast Socks5 Proxy Silent là bản chạy trên docker của [fast-socks5](https://github.com/dizda/fast-socks5) (bản gốc chạy qua Rust)

## Docker vesion

* Cài đặt nhanh, cấu hình tương tự các docker socks thông dụng
* Hiệu năng cao, nhẹ, nhanh
* Hỗ trợ UDP tương tự bản Rust
* Thử nghiệm trên Oracle E2.1.Micro US
  * Khi chạy cường độ cao, dùng ~ **4MB-10MB RAM** và tối đa ~**20% CPU**, trung bình ~**10% CPU**
  * Tốc độ download khi thử nghiệm trên Telegram rất ấn tượng, có khí ~ **7MB/s**
  * Trên Firefox/Chrome/Chromium  trung bình **4MB-6MB/s** 
* Mặc định **tắt tất cả logs từ hệ thống**, bất kể log gì
* Chỉ build phần [server](https://github.com/dizda/fast-socks5/blob/master/examples/server.rs) giúp bản images nhẹ hơn **(\~ 1.5MB)**
* **Beta thử nghiệm `AUTH_ONCE` xác thực 1 lần**

---

### Cài đặt tự động

* Bash script bên dưới tạo cấu hình ngẫu nhiên `PORT`, `USER`, `PASS`, bật `UDP`, tắt thêm **logs từ docker**

```bash
wget -qO fast-socks5.sh https://go.bibica.net/fast-socks5 && sudo bash fast-socks5.sh
```

* Sau ít phút nhận được đầy đủ thông tin sử dụng, đỡ phải nhìn các cấu hình phức tạp

```
🚀 Fast Socks5 Proxy Configuration
  🌐 Server IP: 165.1.64.65
  🚪 Port: 18702
  👤 Username: StpUCK5cFV3q
  🔑 Password: 0kdUMTzeuUUyTTzK
  📡 Protocols: TCP ✅ UDP ✅
  📝 Logging: Disabled
  🔍 Status: Active
```

---

### Cài đặt thủ công qua docker

> **🐳 1. Không xác thực người dùng**

```yaml
services:
  fast-socks5:
    image: bibica/fast-socks5-server-silent
    container_name: fast-socks5
    restart: always
    ports:
      - "2324:2324"
    environment:
      - AUTH_MODE=no-auth
```

* Bật chạy

```bash
docker compose up -d
```

* Kiểm tra

```bash
curl --socks5 localhost:2324 https://ifconfig.me
```

---

> **🐳 2. Xác thực người dùng**

```yaml
services:
  fast-socks5:
    image: bibica/fast-socks5-server-silent
    container_name: fast-socks5
    restart: always
    ports:
      - "2324:2324"
    environment:
      - PROXY_USER=myuser
      - PROXY_PASSWORD=mypass
```

* Kiểm tra

```bash
curl --proxy socks5h://myuser:mypass@127.0.0.1:2324 https://ifconfig.me
```

---

> **🐳 3. Xác thực người dùng + bật hỗ trợ UDP**

```yaml
services:
  fast-socks5:
    image: bibica/fast-socks5-server-silent
    container_name: fast-socks5
    restart: always
    ports:
      - "2324:2324/tcp"
      - "2324:2324/udp"
    environment:
      - PROXY_USER=myuser
      - PROXY_PASSWORD=mypass
      - ALLOW_UDP=true
      - PUBLIC_ADDR=1.2.3.4 # Thay bằng địa chỉ public VPS
```

* Kiểm tra UDP qua đoạn script Python:

```bash
python3 -c "
import socket
user='myuser'
passwd='mypass'
sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
sock.connect(('1.2.3.4',2324))
sock.send(b'\x05\x02\x00\x02')
sock.recv(2)
auth = b'\x01' + bytes([len(user)]) + user.encode() + bytes([len(passwd)]) + passwd.encode()
sock.send(auth)
sock.recv(2)
sock.send(b'\x05\x03\x00\x01\x00\x00\x00\x00\x00\x00')
resp = sock.recv(10)
print('UDP OK' if len(resp) >= 2 and resp[1] == 0 else 'UDP NO')
sock.close()
"
```

* Kết quả trả về sẽ là `UDP OK` hoặc `UDP NO`

---

> **🐳 4. Xác thực người dùng + bật hỗ trợ UDP + ngẫu nhiên tất cả port, user + tắt log docker...**

* Đây là cấu hình dùng trên bản tự động:

```yaml
services:
  fast-socks5:
    image: bibica/fast-socks5-server-silent
    container_name: fast-socks5
    restart: always
    ports:
      - "11135:19563/tcp"
      - "11135:19563/udp"
    environment:
      - PROXY_PORT=19563
      - PROXY_USER=nV8PRJ0kXZmb
      - PROXY_PASSWORD=SMl43P3CstFP2Vmy
      - ALLOW_UDP=true
      - PUBLIC_ADDR=1.2.3.4 # Thay bằng địa chỉ public VPS
    logging:
      driver: "none"
```

> **🐳 5. Xác thực người dùng + hỗ trợ đăng nhập 1 lần + bật hỗ trợ UDP + ngẫu nhiên tất cả port, user + tắt log docker...

Tinh năng chủ yếu chạy cá nhân, bạn nào thích thì thử cho vui, muốn ổn định dùng bản mặc định

* Tính năng beta `AUTH_ONCE=true`: xác thực một lần (học theo `MicroSocks`)
  * Lần kết nối đầu tiên: Client phải xác thực bằng username/password
  * Kết nối thành công: IP được thêm vào whitelist
  * Các lần kết nối sau: Client từ IP đó sẽ được bypass authentication tự động
  * Browsers/Apps: Các ứng dụng không hỗ trợ SOCKS auth có thể truy cập sau khi được whitelist

```yaml
services:
  fast-socks5-auth-once:
    image: bibica/fast-socks5-server-silent-auth-once
    container_name: fast-socks5-auth-once
    restart: always
    ports:
      - "15402:12426/tcp"
      - "15402:12426/udp"
    environment:
      - PROXY_PORT=12426
      - PROXY_USER=StpUCK5cFV3q
      - PROXY_PASSWORD=SMl43P3CstFP2Vmy
      - ALLOW_UDP=true
      - PUBLIC_ADDR=1.2.3.4 # Thay bằng địa chỉ public VPS
      - AUTH_ONCE=true
    logging:
      driver: "none"
```

* Kiểm tra `TCP`, `UDP`, `AUTH_ONCE` qua `socks5_udp_test.py`

```bash
python3 socks5_udp_test.py 1.2.3.4 15402 StpUCK5cFV3q SMl43P3CstFP2Vmy
```
1. Lần đầu sử dụng USERNAME/PASSWORD

```
=== SOCKS5 UDP Associate Test ===
Connecting to 1.2.3.4:12426...
✓ TCP connection established
Sending authentication methods...
✓ Server selected USERNAME/PASSWORD authentication
Sending credentials...
✓ Authentication successful
Sending UDP Associate request...
Waiting for UDP Associate response...
Response: VER=5, REP=0, RSV=0, ATYP=1
✓ UDP Associate successful!
✓ UDP relay server: 1.2.3.4:39904
```

2. Từ lần 2 NO AUTHENTICATION (Bypass authentication tự động)

```
=== SOCKS5 UDP Associate Test ===
Connecting to 1.2.3.4:14268...
✓ TCP connection established
Sending authentication methods...
✓ Server selected NO AUTHENTICATION
Sending UDP Associate request...
Waiting for UDP Associate response...
Response: VER=5, REP=0, RSV=0, ATYP=1
✓ UDP Associate successful!
✓ UDP relay server: 1.2.3.4:52311
```

- `AUTH_ONCE=true` là tính năng thử nghiệm viết thêm so với bản gốc, không chắc chạy có lỗi gì không
- IP đưa vào danh sách trắng được lưu mặc định trong RAM, tất cả IP whitelist sẽ bị mất khi server (container) dừng hoặc crash

---

## Biến môi trường hỗ trợ (Environment Variables)

| Tên biến môi trường | Giá trị mặc định | Bắt buộc | Mô tả                                                           |
| ------------------- | ---------------- | -------- | --------------------------------------------------------------- |
| `PROXY_PORT`        | `2324`           | ❌        | Cổng mà SOCKS5 server sẽ lắng nghe.                             |
| `AUTH_MODE`         | `password`       | ❌        | Chế độ xác thực: `password` (mặc định) hoặc `no-auth`.          |
| `ALLOW_UDP`         | `false`          | ❌        | Cho phép proxy UDP nếu giá trị là `true` và có `PUBLIC_ADDR`.   |
| `PUBLIC_ADDR`       | Không có         | ❌        | Địa chỉ IP công khai, dùng để UDP forwarding.                   |
| `REQUEST_TIMEOUT`   | `10`             | ❌        | Thời gian chờ request tối đa (tính bằng giây).                  |
| `SKIP_AUTH`         | `false`          | ❌        | Bỏ qua kiểm tra xác thực người dùng hoàn toàn nếu bật (`true`). |
| `PROXY_USER`        | Không có         | ✅        | Tên người dùng khi dùng chế độ `password`.                      |
| `PROXY_PASSWORD`    | Không có         | ✅        | Mật khẩu tương ứng với `PROXY_USER`.                            |

---

## Kiểm tra ghi logs

* Mặc định đã tắt, có thể kiểm tra nhanh qua:

```bash
docker stats fast-socks5
```

---

## Tác giả

* [dizda](https://github.com/dizda)
