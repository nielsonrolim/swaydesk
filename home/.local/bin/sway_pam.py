"""Confere a senha do usuário pelo PAM, para o bloqueio de tela, sem GTK.

Chama a libpam.so.0 por ctypes. Não usa o python3-pam: no Debian e no Ubuntu ele é o PyPAM, com
outra API. O serviço é o próprio, /etc/pam.d/swaydesk-lock (só "auth include login", como o do swaylock),
que o swaydesk-session e o install.sh --system trazem; sem ele, o do swaylock (pam_service()).

Só pam_authenticate e, depois de acertar, pam_setcred(PAM_REFRESH_CRED), como o swaylock. Nunca o
pam_acct_mgmt: o arquivo do swaylock só tem "auth", e a conta cairia na pilha "other" da distro,
que pode negar tudo.

As respostas da conversa são alocadas com o malloc/strdup da libc, porque o PAM chama free() nelas:
memória do Python ali daria double free e derrubaria o bloqueio. A senha fica num buffer do ctypes
que é zerado no fim, e as cópias dadas ao PAM são zeradas antes de ele as liberar quando o próprio
módulo devolve erro. Em Python não dá para garantir que nenhuma outra cópia fique na memória.
"""
import ctypes
import ctypes.util
import os
import pwd
from collections import namedtuple

SERVICE = "swaydesk-lock"
LEGACY_SERVICE = "swaylock"  # os dotfiles sem o install.sh --system, com o swaylock da distro
PAM_DIRS = ("/etc/pam.d", "/usr/lib/pam.d")  # a pasta do admin e a dos pacotes (vendordir)


def pam_service(dirs=PAM_DIRS):
    """O próprio, se existir; senão o do swaylock, se existir; sem nenhum, o próprio (o PAM cai no "other")."""
    def has(name):
        return any(os.path.isfile(os.path.join(d, name)) for d in dirs)
    if not has(SERVICE) and has(LEGACY_SERVICE):
        return LEGACY_SERVICE
    return SERVICE

PAM_SUCCESS = 0
PAM_BUF_ERR = 5
PAM_PERM_DENIED = 6
PAM_AUTH_ERR = 7
PAM_AUTHINFO_UNAVAIL = 9
PAM_USER_UNKNOWN = 10
PAM_MAXTRIES = 11
PAM_NEW_AUTHTOK_REQD = 12
PAM_ACCT_EXPIRED = 13
PAM_CONV_ERR = 19
PAM_REFRESH_CRED = 0x0010

PAM_PROMPT_ECHO_OFF = 1
PAM_PROMPT_ECHO_ON = 2
PAM_ERROR_MSG = 3
PAM_TEXT_INFO = 4

# O que a tela mostra: a senha estava errada, a conta não pode entrar, ou o PAM falhou
OK, WRONG, EXPIRED, ERROR = "ok", "wrong", "expired", "error"
WRONG_CODES = {PAM_AUTH_ERR, PAM_USER_UNKNOWN, PAM_MAXTRIES, PAM_PERM_DENIED}
EXPIRED_CODES = {PAM_ACCT_EXPIRED, PAM_NEW_AUTHTOK_REQD}

Result = namedtuple("Result", "status code messages")


class PamMessage(ctypes.Structure):
    _fields_ = [("msg_style", ctypes.c_int), ("msg", ctypes.c_char_p)]


class PamResponse(ctypes.Structure):
    _fields_ = [("resp", ctypes.c_void_p), ("resp_retcode", ctypes.c_int)]


CONV_FUNC = ctypes.CFUNCTYPE(ctypes.c_int, ctypes.c_int, ctypes.POINTER(ctypes.POINTER(PamMessage)),
                             ctypes.POINTER(ctypes.POINTER(PamResponse)), ctypes.c_void_p)


class PamConv(ctypes.Structure):
    _fields_ = [("conv", CONV_FUNC), ("appdata_ptr", ctypes.c_void_p)]


def failure_text(messages, fallback):
    """O que mostrar de uma tentativa recusada: todas as mensagens do PAM, na ordem (o faillock do Arch manda duas,
    "The account is locked due to 3 failed logins." e "(10 minutes left to unlock)"), ou o texto próprio."""
    said = [m.strip() for m in messages if m.strip()]
    return " ".join(said) if said else fallback


def status_of(code):
    if code == PAM_SUCCESS:
        return OK
    if code in WRONG_CODES:
        return WRONG
    if code in EXPIRED_CODES:
        return EXPIRED
    return ERROR


def load_libc():
    libc = ctypes.CDLL(ctypes.util.find_library("c") or "libc.so.6")
    libc.calloc.restype = ctypes.c_void_p
    libc.calloc.argtypes = [ctypes.c_size_t, ctypes.c_size_t]
    libc.strdup.restype = ctypes.c_void_p
    libc.strdup.argtypes = [ctypes.c_void_p]
    libc.free.restype = None
    libc.free.argtypes = [ctypes.c_void_p]
    libc.memset.restype = ctypes.c_void_p
    libc.memset.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_size_t]
    libc.strlen.restype = ctypes.c_size_t
    libc.strlen.argtypes = [ctypes.c_void_p]
    return libc


def load_libpam():
    lib = ctypes.CDLL("libpam.so.0")
    lib.pam_start.restype = ctypes.c_int
    lib.pam_start.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.POINTER(PamConv),
                              ctypes.POINTER(ctypes.c_void_p)]
    for name in ("pam_authenticate", "pam_setcred", "pam_end"):
        func = getattr(lib, name)
        func.restype = ctypes.c_int
        func.argtypes = [ctypes.c_void_p, ctypes.c_int]
    return lib


def current_user():
    """Do getpwuid, nunca do $USER, que quem abriu o programa escolhe."""
    return pwd.getpwuid(os.getuid()).pw_name


def wipe(libc, address):
    """Zera uma string da libc antes de liberá-la."""
    if address:
        libc.memset(address, 0, libc.strlen(address))


def authenticate(password, user=None, service=None, lib=None, libc=None):
    """Result(status, código do PAM, mensagens do PAM). password: bytes ou bytearray; o bytearray é
    zerado no fim. Bloqueia: rodar fora da linha da interface (o PAM pode levar segundos)."""
    libc = libc or load_libc()
    lib = lib or load_libpam()
    user = user or current_user()
    service = service or pam_service()
    secret = ctypes.create_string_buffer(bytes(password))
    if isinstance(password, bytearray):
        password[:] = bytes(len(password))
    messages = []

    def release(replies, array, upto):
        for j in range(upto):
            wipe(libc, replies[j].resp)
            libc.free(replies[j].resp)
        libc.free(array)

    def conversation(count, msgs, resp_out, _appdata):
        # Uma exceção num callback do ctypes só é impressa, e ele devolveria 0 (PAM_SUCCESS) sem resposta
        try:
            return converse(count, msgs, resp_out)
        except Exception:  # noqa: BLE001
            return PAM_CONV_ERR

    def converse(count, msgs, resp_out):
        if count <= 0:
            return PAM_CONV_ERR
        array = libc.calloc(count, ctypes.sizeof(PamResponse))
        if not array:
            return PAM_BUF_ERR
        replies = ctypes.cast(array, ctypes.POINTER(PamResponse))
        try:
            return fill(count, msgs, resp_out, replies, array)
        except Exception:
            release(replies, array, count)
            raise

    def fill(count, msgs, resp_out, replies, array):
        for i in range(count):
            message = msgs[i].contents
            if message.msg_style in (PAM_PROMPT_ECHO_OFF, PAM_PROMPT_ECHO_ON):
                # O swaylock responde aos dois tipos com a senha
                copy = libc.strdup(ctypes.addressof(secret))  # sem passar por bytes do Python
                if not copy:
                    release(replies, array, i)
                    return PAM_BUF_ERR
                replies[i].resp = copy
            elif message.msg_style in (PAM_ERROR_MSG, PAM_TEXT_INFO):
                if message.msg:
                    messages.append(message.msg.decode(errors="replace"))
            else:
                release(replies, array, i)
                return PAM_CONV_ERR
        resp_out[0] = replies
        return PAM_SUCCESS

    callback = CONV_FUNC(conversation)  # referência viva até o pam_end
    conv = PamConv(callback, None)
    handle = ctypes.c_void_p()
    started = False
    code = PAM_CONV_ERR
    try:
        code = lib.pam_start(service.encode(), user.encode(), ctypes.byref(conv), ctypes.byref(handle))
        if code != PAM_SUCCESS:
            return Result(ERROR, code, messages)
        started = True
        code = lib.pam_authenticate(handle, 0)
        if code == PAM_SUCCESS:
            lib.pam_setcred(handle, PAM_REFRESH_CRED)  # renova o Kerberos e afins; o resultado não decide
        return Result(status_of(code), code, messages)
    finally:
        if started:
            lib.pam_end(handle, code)
        ctypes.memset(secret, 0, len(secret))
