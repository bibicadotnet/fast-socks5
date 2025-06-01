[Docker Hub](https://hub.docker.com/r/bibica/fast-socks5-server-silent)
# Fast Socks5 Proxy Silent

Fast SOCKS5 TCP and UDP client/server library for Rust async (Tokio)

- Hiệu năng cao, nhẹ, nhanh, an toàn, và dễ tích hợp 
- Được duy trì bởi anyip.io
- Hỗ trợ UDP
- Phiên bản docker mặc định tắt tất cả logs từ hệ thống

### Cài đặt nhanh tự động, ngẫu nhiên `PORT` `USER` `PASS` và bật `UDP`
```
wget -qO fast-socks5.sh https://go.bibica.net/fast-socks5 && sudo bash fast-socks5.sh
```

### Có thể kiểm tra nhanhn socks đã hỗ trợ UDP
- Nhập các giá trị `IP` `PORT` `USER` `PASS` vào code bên dưới -> Enter
```
python3 -c "
import socket
user='gMBZtE9FktmX'
passwd='5uo00D3Znp6vCdQt'
sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
sock.connect(('165.121.164.165',17201))
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
- Kết quả trả về `UDP NO` hoặc `UDP OK`

# Tác giả
- [dizda](https://github.com/dizda)
