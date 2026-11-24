//! Confere a senha do usuário pelo PAM: o porte do `sway_pam.py`.
//!
//! A libpam.so.0 é aberta na hora (dlopen): sem o pacote -dev para compilar, e sem ligar o binário a
//! ela. O serviço é o próprio, /etc/pam.d/swaydesk-lock (só "auth include login", como o do swaylock), que o
//! swaydesk-lock traz; sem ele (os dotfiles sem o install.sh --system), o do swaylock ([`service`]).
//!
//! Só pam_authenticate e, depois de acertar, pam_setcred(PAM_REFRESH_CRED), como o swaylock. Nunca o
//! pam_acct_mgmt: o arquivo do swaylock só tem "auth", e a conta cairia na pilha "other" da distro, que
//! pode negar tudo.
//!
//! As respostas da conversa são alocadas com o calloc/strdup da libc, porque o PAM chama free() nelas.
//! A senha fica num buffer zerado no fim, e as cópias dadas ao PAM são zeradas antes de liberadas quando
//! a conversa falha no meio.
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::panic::AssertUnwindSafe;

pub const SERVICE: &str = "swaydesk-lock";
/// O de antes, para quem não tem o próprio: os dotfiles sem o install.sh --system, que usam o swaylock da distro.
pub const LEGACY_SERVICE: &str = "swaylock";
/// Onde o Linux-PAM procura os serviços: a pasta do admin e a dos pacotes (vendordir).
pub const PAM_DIRS: &[&str] = &["/etc/pam.d", "/usr/lib/pam.d"];

/// O serviço a usar: o próprio, se o arquivo existir numa das pastas; senão o do swaylock, se existir; sem
/// nenhum dos dois, o próprio, e o PAM cai na pilha "other" da distro, como cairia com o do swaylock.
pub fn service() -> &'static str {
    service_in(PAM_DIRS.iter().map(std::path::Path::new))
}

pub fn service_in<'a>(dirs: impl Iterator<Item = &'a std::path::Path> + Clone) -> &'static str {
    let has = |name: &str| dirs.clone().any(|dir| dir.join(name).is_file());
    if !has(SERVICE) && has(LEGACY_SERVICE) {
        return LEGACY_SERVICE;
    }
    SERVICE
}

pub const PAM_SUCCESS: c_int = 0;
pub const PAM_BUF_ERR: c_int = 5;
pub const PAM_PERM_DENIED: c_int = 6;
pub const PAM_AUTH_ERR: c_int = 7;
pub const PAM_AUTHINFO_UNAVAIL: c_int = 9;
pub const PAM_USER_UNKNOWN: c_int = 10;
pub const PAM_MAXTRIES: c_int = 11;
pub const PAM_NEW_AUTHTOK_REQD: c_int = 12;
pub const PAM_ACCT_EXPIRED: c_int = 13;
pub const PAM_CONV_ERR: c_int = 19;
pub const PAM_REFRESH_CRED: c_int = 0x0010;

pub const PAM_PROMPT_ECHO_OFF: c_int = 1;
pub const PAM_PROMPT_ECHO_ON: c_int = 2;
pub const PAM_ERROR_MSG: c_int = 3;
pub const PAM_TEXT_INFO: c_int = 4;

/// O que a tela mostra: acertou, a senha estava errada, a conta não pode entrar, ou o PAM falhou.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    Wrong,
    Expired,
    Error,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub status: Status,
    pub code: c_int,
    pub messages: Vec<String>,
}

/// O que mostrar de uma tentativa recusada: todas as mensagens do PAM, na ordem (o faillock do Arch manda duas,
/// "The account is locked due to 3 failed logins." e "(10 minutes left to unlock)"), ou o texto próprio.
pub fn failure_text(messages: &[String], fallback: String) -> String {
    let said: Vec<&str> = messages.iter().map(|m| m.trim()).filter(|m| !m.is_empty()).collect();
    if said.is_empty() {
        fallback
    } else {
        said.join(" ")
    }
}

pub fn status_of(code: c_int) -> Status {
    match code {
        PAM_SUCCESS => Status::Ok,
        PAM_AUTH_ERR | PAM_USER_UNKNOWN | PAM_MAXTRIES | PAM_PERM_DENIED => Status::Wrong,
        PAM_ACCT_EXPIRED | PAM_NEW_AUTHTOK_REQD => Status::Expired,
        _ => Status::Error,
    }
}

#[repr(C)]
pub struct PamMessage {
    pub msg_style: c_int,
    pub msg: *const c_char,
}

#[repr(C)]
pub struct PamResponse {
    pub resp: *mut c_char,
    pub resp_retcode: c_int,
}

pub type ConvFn = unsafe extern "C" fn(c_int, *mut *const PamMessage, *mut *mut PamResponse, *mut c_void) -> c_int;

#[repr(C)]
pub struct PamConv {
    pub conv: Option<ConvFn>,
    pub appdata_ptr: *mut c_void,
}

/// As quatro funções da libpam que o bloqueio usa. Uma implementação falsa nos testes.
pub trait Pam {
    fn start(&self, service: &CStr, user: &CStr, conv: &PamConv, handle: &mut *mut c_void) -> c_int;
    fn authenticate(&self, handle: *mut c_void, flags: c_int) -> c_int;
    fn setcred(&self, handle: *mut c_void, flags: c_int) -> c_int;
    fn end(&self, handle: *mut c_void, status: c_int) -> c_int;
}

type StartFn = unsafe extern "C" fn(*const c_char, *const c_char, *const PamConv, *mut *mut c_void) -> c_int;
type CallFn = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;

/// A libpam.so.0 de verdade.
pub struct LibPam {
    start: StartFn,
    authenticate: CallFn,
    setcred: CallFn,
    end: CallFn,
}

impl LibPam {
    pub fn open() -> Result<LibPam, String> {
        unsafe {
            let lib = libc::dlopen(c"libpam.so.0".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
            if lib.is_null() {
                return Err("could not open libpam.so.0".into());
            }
            let find = |name: &CStr| {
                let symbol = libc::dlsym(lib, name.as_ptr());
                if symbol.is_null() { Err(format!("libpam.so.0 has no {}", name.to_string_lossy())) } else { Ok(symbol) }
            };
            Ok(LibPam {
                start: std::mem::transmute::<*mut c_void, StartFn>(find(c"pam_start")?),
                authenticate: std::mem::transmute::<*mut c_void, CallFn>(find(c"pam_authenticate")?),
                setcred: std::mem::transmute::<*mut c_void, CallFn>(find(c"pam_setcred")?),
                end: std::mem::transmute::<*mut c_void, CallFn>(find(c"pam_end")?),
            })
        }
    }
}

impl Pam for LibPam {
    fn start(&self, service: &CStr, user: &CStr, conv: &PamConv, handle: &mut *mut c_void) -> c_int {
        unsafe { (self.start)(service.as_ptr(), user.as_ptr(), conv, handle) }
    }
    fn authenticate(&self, handle: *mut c_void, flags: c_int) -> c_int {
        unsafe { (self.authenticate)(handle, flags) }
    }
    fn setcred(&self, handle: *mut c_void, flags: c_int) -> c_int {
        unsafe { (self.setcred)(handle, flags) }
    }
    fn end(&self, handle: *mut c_void, status: c_int) -> c_int {
        unsafe { (self.end)(handle, status) }
    }
}

/// O que a conversa recebe: a senha (terminada em NUL) e as mensagens que o PAM mostrou.
struct Talk {
    secret: *const c_char,
    messages: Vec<String>,
}

/// Zera uma string da libc (antes de liberá-la).
unsafe fn wipe(text: *mut c_char) {
    if !text.is_null() {
        let n = libc::strlen(text);
        for i in 0..n {
            std::ptr::write_volatile(text.add(i), 0);
        }
    }
}

unsafe fn release(array: *mut PamResponse, upto: usize) {
    for j in 0..upto {
        let reply = &mut *array.add(j);
        wipe(reply.resp);
        libc::free(reply.resp.cast());
    }
    libc::free(array.cast());
}

/// A conversa: a senha para os dois tipos de pergunta (como o swaylock), e guarda os avisos. Um pânico
/// aqui vira erro de conversa, nunca um "acertou".
unsafe extern "C" fn conversation(count: c_int, msgs: *mut *const PamMessage, out: *mut *mut PamResponse, appdata: *mut c_void) -> c_int {
    std::panic::catch_unwind(AssertUnwindSafe(|| converse(count, msgs, out, appdata))).unwrap_or(PAM_CONV_ERR)
}

unsafe fn converse(count: c_int, msgs: *mut *const PamMessage, out: *mut *mut PamResponse, appdata: *mut c_void) -> c_int {
    if count <= 0 || msgs.is_null() || out.is_null() || appdata.is_null() {
        return PAM_CONV_ERR;
    }
    let talk = &mut *(appdata as *mut Talk);
    let count = count as usize;
    let array = libc::calloc(count, std::mem::size_of::<PamResponse>()) as *mut PamResponse;
    if array.is_null() {
        return PAM_BUF_ERR;
    }
    for i in 0..count {
        // No Linux-PAM, msgs é um vetor de ponteiros
        let message = *msgs.add(i);
        if message.is_null() {
            release(array, i);
            return PAM_CONV_ERR;
        }
        let message = &*message;
        match message.msg_style {
            PAM_PROMPT_ECHO_OFF | PAM_PROMPT_ECHO_ON => {
                let copy = libc::strdup(talk.secret);
                if copy.is_null() {
                    release(array, i);
                    return PAM_BUF_ERR;
                }
                (*array.add(i)).resp = copy;
            }
            PAM_ERROR_MSG | PAM_TEXT_INFO => {
                if !message.msg.is_null() {
                    talk.messages.push(CStr::from_ptr(message.msg).to_string_lossy().into_owned());
                }
            }
            _ => {
                release(array, i);
                return PAM_CONV_ERR;
            }
        }
    }
    *out = array;
    PAM_SUCCESS
}

/// Confere a senha. Zera `password` na hora e o buffer interno no fim. Bloqueia: rodar fora da linha da
/// interface (o PAM pode levar segundos).
pub fn authenticate(pam: &dyn Pam, password: &mut Vec<u8>, user: &str, service: &str) -> Outcome {
    // A senha até o primeiro NUL, como o strdup a veria, num buffer só nosso
    let mut secret: Vec<u8> = Vec::with_capacity(password.len() + 1);
    secret.extend(password.iter().take_while(|&&b| b != 0));
    secret.push(0);
    for byte in password.iter_mut() {
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
    let mut talk = Talk { secret: secret.as_ptr().cast(), messages: Vec::new() };
    let outcome = run(pam, &mut talk, user, service);
    for byte in secret.iter_mut() {
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
    outcome
}

fn run(pam: &dyn Pam, talk: &mut Talk, user: &str, service: &str) -> Outcome {
    let error = |code, messages| Outcome { status: Status::Error, code, messages };
    let (Ok(user), Ok(service)) = (CString::new(user), CString::new(service)) else {
        return error(PAM_CONV_ERR, Vec::new());
    };
    let conv = PamConv { conv: Some(conversation), appdata_ptr: (talk as *mut Talk).cast() };
    let mut handle: *mut c_void = std::ptr::null_mut();
    let code = pam.start(&service, &user, &conv, &mut handle);
    if code != PAM_SUCCESS {
        return error(code, std::mem::take(&mut talk.messages));
    }
    let code = pam.authenticate(handle, 0);
    if code == PAM_SUCCESS {
        pam.setcred(handle, PAM_REFRESH_CRED); // renova o Kerberos e afins; o resultado não decide
    }
    pam.end(handle, code);
    Outcome { status: status_of(code), code, messages: std::mem::take(&mut talk.messages) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pam_message_is_shown_and_without_any_our_own_text() {
        let locked = vec!["The account is locked due to 3 failed logins.".to_string(), "(10 minutes left to unlock)".to_string()];
        assert_eq!(
            failure_text(&locked, "Wrong password".into()),
            "The account is locked due to 3 failed logins. (10 minutes left to unlock)"
        );
        assert_eq!(failure_text(&[], "Wrong password".into()), "Wrong password");
        assert_eq!(failure_text(&[" ".into()], "Wrong password".into()), "Wrong password");
    }
    use std::cell::RefCell;

    /// Um PAM falso: na autenticação, faz as perguntas `styles` pela conversa, guarda as respostas e
    /// as libera com o free(), como o PAM de verdade.
    struct FakePam {
        styles: Vec<(c_int, Option<&'static CStr>)>,
        result: c_int,
        start_result: c_int,
        conv: RefCell<Option<(ConvFn, *mut c_void)>>,
        answers: RefCell<Vec<Option<Vec<u8>>>>,
        calls: RefCell<Vec<(String, c_int)>>,
        start_args: RefCell<Option<(String, String)>>,
    }

    impl FakePam {
        fn new(result: c_int) -> FakePam {
            FakePam {
                styles: vec![(PAM_PROMPT_ECHO_OFF, Some(c"Password: "))],
                result,
                start_result: PAM_SUCCESS,
                conv: RefCell::new(None),
                answers: RefCell::new(Vec::new()),
                calls: RefCell::new(Vec::new()),
                start_args: RefCell::new(None),
            }
        }
        fn names(&self) -> Vec<String> {
            self.calls.borrow().iter().map(|c| c.0.clone()).collect()
        }
    }

    impl Pam for FakePam {
        fn start(&self, service: &CStr, user: &CStr, conv: &PamConv, handle: &mut *mut c_void) -> c_int {
            self.calls.borrow_mut().push(("start".into(), 0));
            *self.start_args.borrow_mut() = Some((service.to_string_lossy().into(), user.to_string_lossy().into()));
            *self.conv.borrow_mut() = Some((conv.conv.unwrap(), conv.appdata_ptr));
            *handle = 1 as *mut c_void;
            self.start_result
        }
        fn authenticate(&self, _handle: *mut c_void, _flags: c_int) -> c_int {
            self.calls.borrow_mut().push(("authenticate".into(), 0));
            let (conv, appdata) = self.conv.borrow().unwrap();
            let messages: Vec<PamMessage> = self.styles.iter().map(|(style, text)| PamMessage { msg_style: *style, msg: text.map_or(std::ptr::null(), |t| t.as_ptr()) }).collect();
            let mut pointers: Vec<*const PamMessage> = messages.iter().map(|m| m as *const PamMessage).collect();
            let mut out: *mut PamResponse = std::ptr::null_mut();
            let rc = unsafe { conv(pointers.len() as c_int, pointers.as_mut_ptr(), &mut out, appdata) };
            if rc != PAM_SUCCESS {
                return rc;
            }
            unsafe {
                for i in 0..pointers.len() {
                    let resp = (*out.add(i)).resp;
                    self.answers.borrow_mut().push((!resp.is_null()).then(|| CStr::from_ptr(resp).to_bytes().to_vec()));
                    libc::free(resp.cast());
                }
                libc::free(out.cast());
            }
            self.result
        }
        fn setcred(&self, _handle: *mut c_void, flags: c_int) -> c_int {
            self.calls.borrow_mut().push(("setcred".into(), flags));
            PAM_SUCCESS
        }
        fn end(&self, _handle: *mut c_void, status: c_int) -> c_int {
            self.calls.borrow_mut().push(("end".into(), status));
            PAM_SUCCESS
        }
    }

    fn auth(fake: &FakePam) -> Outcome {
        authenticate(fake, &mut b"certa".to_vec(), "ana", SERVICE)
    }

    #[test]
    fn right_password() {
        let fake = FakePam::new(PAM_SUCCESS);
        assert_eq!(auth(&fake).status, Status::Ok);
        assert_eq!(*fake.answers.borrow(), vec![Some(b"certa".to_vec())]);
        assert!(fake.calls.borrow().contains(&("setcred".into(), PAM_REFRESH_CRED)));
        assert_eq!(fake.calls.borrow().last(), Some(&("end".into(), PAM_SUCCESS)));
    }

    #[test]
    fn wrong_password_never_sets_credentials() {
        let fake = FakePam::new(PAM_AUTH_ERR);
        let result = auth(&fake);
        assert_eq!((result.status, result.code), (Status::Wrong, PAM_AUTH_ERR));
        assert_eq!(fake.names(), ["start", "authenticate", "end"]);
        assert_eq!(fake.calls.borrow().last(), Some(&("end".into(), PAM_AUTH_ERR)));
    }

    #[test]
    fn expired_account_and_pam_errors() {
        assert_eq!(auth(&FakePam::new(PAM_ACCT_EXPIRED)).status, Status::Expired);
        assert_eq!(auth(&FakePam::new(PAM_NEW_AUTHTOK_REQD)).status, Status::Expired);
        assert_eq!(auth(&FakePam::new(PAM_AUTHINFO_UNAVAIL)).status, Status::Error);
        let mut fake = FakePam::new(PAM_SUCCESS);
        fake.start_result = PAM_BUF_ERR;
        assert_eq!(auth(&fake).status, Status::Error);
        assert_eq!(fake.names(), ["start"]);
    }

    #[test]
    fn both_prompt_styles_get_the_password_and_info_is_kept() {
        let mut fake = FakePam::new(PAM_SUCCESS);
        fake.styles = vec![(PAM_TEXT_INFO, Some(c"Conta bloqueada por 10 s")), (PAM_PROMPT_ECHO_OFF, None), (PAM_PROMPT_ECHO_ON, None)];
        let result = auth(&fake);
        assert_eq!(result.status, Status::Ok);
        assert_eq!(*fake.answers.borrow(), vec![None, Some(b"certa".to_vec()), Some(b"certa".to_vec())]);
        assert_eq!(result.messages, ["Conta bloqueada por 10 s"]);
    }

    #[test]
    fn unknown_style_is_a_conversation_error() {
        let mut fake = FakePam::new(PAM_SUCCESS);
        fake.styles = vec![(PAM_PROMPT_ECHO_OFF, None), (99, None)];
        let result = auth(&fake);
        assert_eq!((result.status, result.code), (Status::Error, PAM_CONV_ERR));
        assert_eq!(fake.calls.borrow().last().map(|c| c.0.as_str()), Some("end"));
    }

    #[test]
    fn user_and_service() {
        let fake = FakePam::new(PAM_SUCCESS);
        auth(&fake);
        assert_eq!(*fake.start_args.borrow(), Some((SERVICE.to_string(), "ana".to_string())));
    }

    #[test]
    fn the_password_is_zeroed() {
        let mut secret = b"certa".to_vec();
        assert_eq!(authenticate(&FakePam::new(PAM_SUCCESS), &mut secret, "ana", SERVICE).status, Status::Ok);
        assert_eq!(secret, vec![0; 5]);
    }

    #[test]
    fn many_rounds_do_not_break_the_heap() {
        for _ in 0..200 {
            let mut fake = FakePam::new(PAM_SUCCESS);
            fake.styles = vec![(PAM_PROMPT_ECHO_OFF, None), (PAM_ERROR_MSG, Some(c"x")), (PAM_PROMPT_ECHO_ON, None)];
            assert_eq!(auth(&fake).status, Status::Ok);
        }
    }

    #[test]
    fn the_real_library_opens() {
        // O dlopen e os quatro símbolos; nada de autenticar
        assert!(LibPam::open().is_ok());
    }

    #[test]
    fn the_own_service_comes_first_and_swaylock_only_without_it() {
        let dir = std::env::temp_dir().join(format!("pam-service-{}", std::process::id()));
        let (etc, vendor) = (dir.join("etc"), dir.join("lib"));
        std::fs::create_dir_all(&etc).unwrap();
        std::fs::create_dir_all(&vendor).unwrap();
        let dirs = [etc.as_path(), vendor.as_path()];
        assert_eq!(service_in(dirs.iter().copied()), SERVICE, "neither: ours, and the distro's other stack");
        std::fs::write(etc.join(LEGACY_SERVICE), "auth include login\n").unwrap();
        assert_eq!(service_in(dirs.iter().copied()), LEGACY_SERVICE);
        std::fs::write(vendor.join(SERVICE), "auth include login\n").unwrap();
        assert_eq!(service_in(dirs.iter().copied()), SERVICE, "in the vendor dir too");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
