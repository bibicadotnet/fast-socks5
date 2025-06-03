#!/usr/bin/env python3
import socket
import struct
import sys

def test_socks5_udp(host, port, username, password):
    """Test SOCKS5 UDP Associate functionality"""
    
    try:
        # Step 1: TCP connection to SOCKS5 server
        print(f"Connecting to {host}:{port}...")
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(10)  # 10 second timeout
        sock.connect((host, port))
        print("✓ TCP connection established")
        
        # Step 2: Authentication negotiation
        print("Sending authentication methods...")
        # Send: VER=5, NMETHODS=2, METHODS=[0x00 (no auth), 0x02 (username/password)]
        sock.send(b'\x05\x02\x00\x02')
        
        # Receive authentication method selection
        auth_response = sock.recv(2)
        if len(auth_response) != 2:
            print("✗ Invalid authentication response length")
            return False
            
        if auth_response[0] != 0x05:
            print("✗ Invalid SOCKS version in auth response")
            return False
            
        if auth_response[1] == 0x00:
            print("✓ Server selected NO AUTHENTICATION")
        elif auth_response[1] == 0x02:
            print("✓ Server selected USERNAME/PASSWORD authentication")
            
            # Step 3: Username/Password authentication
            print("Sending credentials...")
            username_bytes = username.encode('utf-8')
            password_bytes = password.encode('utf-8')
            
            # Format: VER=1, ULEN, USERNAME, PLEN, PASSWORD
            auth_request = struct.pack('B', 1)  # Version
            auth_request += struct.pack('B', len(username_bytes)) + username_bytes
            auth_request += struct.pack('B', len(password_bytes)) + password_bytes
            
            sock.send(auth_request)
            
            # Receive authentication result
            auth_result = sock.recv(2)
            if len(auth_result) != 2 or auth_result[1] != 0x00:
                print("✗ Authentication failed")
                return False
            print("✓ Authentication successful")
        else:
            print(f"✗ Server selected unsupported method: 0x{auth_response[1]:02x}")
            return False
        
        # Step 4: UDP Associate request
        print("Sending UDP Associate request...")
        # Format: VER=5, CMD=3 (UDP Associate), RSV=0, ATYP=1 (IPv4), DST.ADDR=0.0.0.0, DST.PORT=0
        udp_request = struct.pack('!BBBB4sH', 
                                 0x05,  # VER
                                 0x03,  # CMD (UDP Associate)
                                 0x00,  # RSV
                                 0x01,  # ATYP (IPv4)
                                 socket.inet_aton('0.0.0.0'),  # DST.ADDR
                                 0)     # DST.PORT
        
        sock.send(udp_request)
        
        # Receive UDP Associate response
        print("Waiting for UDP Associate response...")
        response = sock.recv(10)
        
        if len(response) < 6:
            print(f"✗ Response too short: {len(response)} bytes")
            return False
            
        ver, rep, rsv, atyp = struct.unpack('!BBBB', response[:4])
        
        print(f"Response: VER={ver}, REP={rep}, RSV={rsv}, ATYP={atyp}")
        
        if ver != 0x05:
            print(f"✗ Invalid SOCKS version: {ver}")
            return False
            
        if rep == 0x00:
            print("✓ UDP Associate successful!")
            
            # Parse the UDP relay server address
            if atyp == 0x01:  # IPv4
                if len(response) >= 10:
                    relay_ip = socket.inet_ntoa(response[4:8])
                    relay_port = struct.unpack('!H', response[8:10])[0]
                    print(f"✓ UDP relay server: {relay_ip}:{relay_port}")
                else:
                    print("✗ Incomplete IPv4 address in response")
            elif atyp == 0x03:  # Domain name
                domain_len = response[4]
                if len(response) >= 5 + domain_len + 2:
                    domain = response[5:5+domain_len].decode('utf-8')
                    relay_port = struct.unpack('!H', response[5+domain_len:7+domain_len])[0]
                    print(f"✓ UDP relay server: {domain}:{relay_port}")
                else:
                    print("✗ Incomplete domain name in response")
            elif atyp == 0x04:  # IPv6
                if len(response) >= 22:
                    relay_ip = socket.inet_ntop(socket.AF_INET6, response[4:20])
                    relay_port = struct.unpack('!H', response[20:22])[0]
                    print(f"✓ UDP relay server: [{relay_ip}]:{relay_port}")
                else:
                    print("✗ Incomplete IPv6 address in response")
            
            return True
            
        else:
            error_messages = {
                0x01: "General SOCKS server failure",
                0x02: "Connection not allowed by ruleset", 
                0x03: "Network unreachable",
                0x04: "Host unreachable",
                0x05: "Connection refused",
                0x06: "TTL expired",
                0x07: "Command not supported",
                0x08: "Address type not supported"
            }
            error_msg = error_messages.get(rep, f"Unknown error code: 0x{rep:02x}")
            print(f"✗ UDP Associate failed: {error_msg}")
            return False
            
    except ConnectionResetError:
        print("✗ Connection reset by peer - server may not support UDP or authentication failed")
        return False
    except socket.timeout:
        print("✗ Connection timeout")
        return False
    except Exception as e:
        print(f"✗ Error: {e}")
        return False
    finally:
        try:
            sock.close()
        except:
            pass

def main():
    if len(sys.argv) != 5:
        print("Usage: python3 socks5_udp_test.py <host> <port> <username> <password>")
        print("Example: python3 socks5_udp_test.py 127.0.0.1 1080 admin password")
        sys.exit(1)
    
    host = sys.argv[1]
    port = int(sys.argv[2])
    username = sys.argv[3]
    password = sys.argv[4]
    
    print("=== SOCKS5 UDP Associate Test ===")
    success = test_socks5_udp(host, port, username, password)
    
    if success:
        print("\n✓ UDP support is working!")
        sys.exit(0)
    else:
        print("\n✗ UDP support test failed")
        sys.exit(1)

if __name__ == "__main__":
    main()
