#!/usr/bin/env python3
"""Um greetd de mentira para o headless-greeter.sh: o socket do greetd-ipc(7), com contas fixas.

- ana e bia: a senha a123;
- otp: a senha a123 e depois um código visível (42), como um segundo fator;
- outra conta: a pergunta da senha e depois auth_error, como o PAM faz com quem não existe.

Cada pedido recebido vai como uma linha JSON para o arquivo do segundo argumento, com a senha trocada por
"***" (o teste confere só que ela chegou). O start_session sai com sucesso.

uso: fake-greetd.py SOCKET LOG
"""
import json
import os
import socket
import struct
import sys

PASSWORD = "a123"
# A conta "acento" tem senha com acento: só entra se as teclas mortas compuserem (´ + e → é)
ACCENT_PASSWORD = "café"


def send(conn, message):
    body = json.dumps(message).encode()
    conn.sendall(struct.pack("=I", len(body)) + body)


def receive(conn):
    head = conn.recv(4, socket.MSG_WAITALL)
    if len(head) < 4:
        return None
    (size,) = struct.unpack("=I", head)
    return json.loads(conn.recv(size, socket.MSG_WAITALL))


def secret(text="Password: "):
    return {"type": "auth_message", "auth_message_type": "secret", "auth_message": text}


def serve(conn, log):
    user, stage = None, None
    while True:
        request = receive(conn)
        if request is None:
            return
        shown = dict(request)
        if request.get("type") == "post_auth_message_response" and stage == "password":
            shown["response"] = "***" if request.get("response") == PASSWORD else "wrong"
        log.write(json.dumps(shown) + "\n")
        log.flush()
        kind = request.get("type")
        if kind == "create_session":
            if user is not None:
                send(conn, {"type": "error", "error_type": "error", "description": "a session is already being configured"})
                continue
            user, stage = request.get("username"), "password"
            send(conn, secret())
        elif kind == "post_auth_message_response":
            if stage == "password":
                if (user in ("ana", "bia", "otp") and request.get("response") == PASSWORD) or \
                        (user == "acento" and request.get("response") == ACCENT_PASSWORD):
                    if user == "otp":
                        stage = "code"
                        send(conn, {"type": "auth_message", "auth_message_type": "info", "auth_message": "A code was sent to your phone"})
                        continue
                    stage = "done"
                    send(conn, {"type": "success"})
                else:
                    user, stage = None, None
                    send(conn, {"type": "error", "error_type": "auth_error", "description": "pam_authenticate: AUTH_ERR"})
            elif stage == "code":
                # O "ok" da mensagem info: agora a pergunta visível
                stage = "code-asked"
                send(conn, {"type": "auth_message", "auth_message_type": "visible", "auth_message": "Verification code: "})
            elif stage == "code-asked":
                if request.get("response") == "42":
                    stage = "done"
                    send(conn, {"type": "success"})
                else:
                    user, stage = None, None
                    send(conn, {"type": "error", "error_type": "auth_error", "description": "bad code"})
            else:
                send(conn, {"type": "error", "error_type": "error", "description": "no question pending"})
        elif kind == "start_session":
            send(conn, {"type": "success"} if stage == "done" else {"type": "error", "error_type": "error", "description": "not authenticated"})
        elif kind == "cancel_session":
            user, stage = None, None
            send(conn, {"type": "success"})
        else:
            send(conn, {"type": "error", "error_type": "error", "description": "unknown request"})


def main():
    path, log_path = sys.argv[1], sys.argv[2]
    if os.path.exists(path):
        os.unlink(path)
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    server.bind(path)
    server.listen(1)
    with open(log_path, "a") as log:
        while True:
            conn, _ = server.accept()
            with conn:
                serve(conn, log)


if __name__ == "__main__":
    main()
