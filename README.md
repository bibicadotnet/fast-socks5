[Docker Hub](https://hub.docker.com/r/bibica/fast-socks5-server-silent)

# Fast Socks5 Proxy Silent

Fast SOCKS5 TCP and UDP client/server library for Rust async (Tokio)

Fast Socks5 Proxy Silent là bản chạy trên docker của [fast-socks5](https://github.com/dizda/fast-socks5) (bản gốc chạy qua Rust)

## Docker vesion

* Cài đặt nhanh, cấu hình tương tự các docker socks thông dụng
* Hiệu năng cao, nhẹ, nhanh
* Hỗ trợ UDP tương tự bản Rust
* Thử nghiệm trên Oracle E2.1.Micro, khi chạy cường độ cao, dùng ~ **4MB RAM** và tối đa ~2**0% CPU**, trung bình ~**10% CPU**
* Sử dụng tài nguyên RAM, CPU khá ít so với các sản phẩm khác
* Mặc định **tắt tất cả logs từ hệ thống**, bất kể log gì
* Chỉ build phần [server](https://github.com/dizda/fast-socks5/blob/master/examples/server.rs) giúp bản images nhẹ hơn **(\~ 1.5MB)**

---

### Cài đặt tự động

* Bash script bên dưới tạo cấu hình ngẫu nhiên `PORT`, `USER`, `PASS`, bật `UDP`, tắt thêm **logs từ docker**

```bash
wget -qO fast-socks5.sh https://go.bibica.net/fast-socks5 && sudo bash fast-socks5.sh
```

* Sau ít phút nhận được đầy đủ thông tin sử dụng, đỡ phải nhìn các cấu hình phức tạp

```
🚀 Fast Socks5 Proxy Information:
  🌐 Server IP: 165.123.164.165
  🚪 Port: 11135
  👤 Username: nV8PRJ0kXZmb
  🔑 Password: SMl43P3CstFP2Vmy
  📡 Protocols: TCP ✅ UDP ✅
  📝 Logging: Disabled
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
