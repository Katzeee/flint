"""A Python protocol peer. Rust creates fixtures and verifies returned messages."""
from pathlib import Path
import socket
import sys

sys.path.insert(0, sys.argv[1])
from flint_protocol.framing import encode_frame, read_frame


class FileSocket:
    def __init__(self, stream):
        self.stream = stream

    def recv(self, count):
        return self.stream.read(count)


with socket.create_connection(("127.0.0.1", int(sys.argv[2])), timeout=5) as connection:
    with open(sys.argv[3], "rb") as source, open(sys.argv[4], "wb") as output:
        while True:
            message = read_frame(FileSocket(source))
            if message is None:
                break
            frame = encode_frame(message)
            for part in (frame[:1], frame[1:3], frame[3:7], frame[7:]):
                connection.sendall(part)
            response = read_frame(connection)
            if response is None:
                raise EOFError("missing response")
            output.write(encode_frame(response))
