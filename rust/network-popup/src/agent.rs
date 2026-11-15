//! O agente de senhas do iwd (`net.connman.iwd.Agent`), registrado só enquanto o popup está aberto e só
//! no backend iwd. O iwd chama o `RequestPassphrase` quando um `Network.Connect` deste popup encontra uma
//! rede nova com senha. O campo aparece no popup e o texto vai só na resposta do D-Bus. Não vai para
//! argv, log ou disco, e o campo é limpo logo depois. O `GtkPasswordEntry` guarda o texto em memória que
//! não vai para o swap.
//!
//! Só o iwd pode chamar: o remetente de cada chamada é conferido contra o dono atual do nome, lido antes
//! do registro. Os pedidos das redes corporativas (chave privada, usuário e senha) voltam como cancelados,
//! com o aviso de que não são suportadas.
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use gtk4::prelude::*;
use gtk4::{gio, glib};
use swaydesk_common::i18n::tr;
use swaydesk_nm::standalone as iwd;
use swaydesk_ui::label;
use swaydesk_ui::list::error_text;

const PATH: &str = "/org/swaydesk/NetworkPopup/Agent";
const CANCELED: &str = "net.connman.iwd.Agent.Error.Canceled";
const CALL_MS: i32 = 10_000;
/// O SSID tem até 32 bytes; um nome mais longo (de um iwd defeituoso, ou bytes inválidos) é cortado
const SSID_MAX: usize = 32;

const XML: &str = r#"<node>
  <interface name="net.connman.iwd.Agent">
    <method name="Release"/>
    <method name="RequestPassphrase"><arg name="network" type="o" direction="in"/><arg name="passphrase" type="s" direction="out"/></method>
    <method name="RequestPrivateKeyPassphrase"><arg name="network" type="o" direction="in"/><arg name="passphrase" type="s" direction="out"/></method>
    <method name="RequestUserNameAndPassword"><arg name="network" type="o" direction="in"/><arg name="user" type="s" direction="out"/><arg name="password" type="s" direction="out"/></method>
    <method name="RequestUserPassword"><arg name="network" type="o" direction="in"/><arg name="user" type="s" direction="in"/><arg name="password" type="s" direction="out"/></method>
    <method name="Cancel"><arg name="reason" type="s" direction="in"/></method>
  </interface>
</node>"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    Off,
    Registering,
    Registered,
    /// O iwd recusou o registro porque outro agente já está registrado
    Taken,
    /// O iwd recusou por outro motivo (o aviso já foi mostrado); não tenta de novo até o iwd reiniciar
    Failed,
}

pub enum Event {
    /// O iwd pediu a senha desta rede (o caminho do Network)
    Asking(String),
    /// O pedido acabou: respondido, cancelado ou desistido pelo iwd
    Done,
    /// Um pedido de rede corporativa, recusado
    Enterprise,
    State(AgentState),
    /// O registro falhou; a mensagem do iwd
    Failed(String),
}

struct Inner {
    bus: RefCell<Option<gio::DBusConnection>>,
    state: Cell<AgentState>,
    /// O nome único do iwd no registro: só ele pode chamar
    owner: RefCell<String>,
    pending: RefCell<Option<gio::DBusMethodInvocation>>,
    /// Uma senha foi dada desde o último pedido: uma falha logo depois é, quase sempre, a senha errada
    answered: Cell<bool>,
    registration: RefCell<Option<gio::RegistrationId>>,
    on_event: RefCell<Option<Rc<dyn Fn(Event)>>>,
    title: gtk4::Label,
    entry: gtk4::PasswordEntry,
}

pub struct Agent {
    /// A página do campo de senha, no Stack do popup
    pub page: gtk4::Box,
    inner: Rc<Inner>,
}

/// O SSID para mostrar: sem caracteres de controle e com até 32 caracteres. O rótulo é texto puro, sem
/// marcação.
pub fn shown_ssid(ssid: &str) -> String {
    ssid.chars().filter(|c| !c.is_control()).take(SSID_MAX).collect()
}

impl Inner {
    fn emit(&self, event: Event) {
        // Clonado antes de chamar: o tratador pode chamar de volta o agente
        let handler = self.on_event.borrow().clone();
        if let Some(handler) = handler {
            handler(event);
        }
    }

    fn set_state(&self, state: AgentState) {
        self.state.set(state);
        self.emit(Event::State(state));
    }

    /// Responde "cancelado" ao pedido em aberto, se houver, e limpa o campo.
    fn cancel_pending(&self) -> bool {
        self.entry.set_text("");
        match self.pending.borrow_mut().take() {
            Some(invocation) => {
                invocation.return_dbus_error(CANCELED, "canceled by the user");
                true
            }
            None => false,
        }
    }

    fn submit(&self) {
        let Some(invocation) = self.pending.borrow_mut().take() else { return };
        let text = self.entry.text();
        self.entry.set_text("");
        invocation.return_value(Some(&(text.as_str(),).to_variant()));
        drop(text);
        self.answered.set(true);
        self.emit(Event::Done);
    }

    fn method_call(&self, sender: Option<&str>, method: &str, params: &glib::Variant, invocation: gio::DBusMethodInvocation) {
        if sender.is_none() || sender != Some(self.owner.borrow().as_str()) || self.owner.borrow().is_empty() {
            invocation.return_dbus_error("org.freedesktop.DBus.Error.AccessDenied", "only iwd may call this agent");
            return;
        }
        match method {
            "RequestPassphrase" => {
                let network = params.child_value(0).str().unwrap_or_default().to_string();
                // Um pedido novo substitui o anterior (o iwd não faz dois de uma vez, mas não custa)
                self.cancel_pending();
                self.answered.set(false);
                self.pending.replace(Some(invocation));
                self.emit(Event::Asking(network));
            }
            "RequestPrivateKeyPassphrase" | "RequestUserNameAndPassword" | "RequestUserPassword" => {
                invocation.return_dbus_error(CANCELED, "enterprise networks are not supported");
                self.emit(Event::Enterprise);
            }
            "Cancel" => {
                // O iwd desistiu do pedido; não espera resposta a ele
                self.entry.set_text("");
                drop(self.pending.borrow_mut().take());
                invocation.return_value(None);
                self.emit(Event::Done);
            }
            "Release" => {
                self.entry.set_text("");
                drop(self.pending.borrow_mut().take());
                invocation.return_value(None);
                self.set_state(AgentState::Off);
                self.emit(Event::Done);
            }
            _ => invocation.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", method),
        }
    }
}

impl Agent {
    pub fn new() -> Agent {
        let page = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
        page.set_valign(gtk4::Align::Center);
        let title = label(" ", &["dev-name"], 0.0);
        title.set_wrap(true);
        title.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
        page.append(&title);
        let entry = gtk4::PasswordEntry::new();
        entry.set_show_peek_icon(true);
        page.append(&entry);
        let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        buttons.set_halign(gtk4::Align::End);
        let cancel = gtk4::Button::with_label(&tr("Cancel"));
        let connect = gtk4::Button::with_label(&tr("Connect"));
        connect.add_css_class("suggested-action");
        buttons.append(&cancel);
        buttons.append(&connect);
        page.append(&buttons);

        let inner = Rc::new(Inner {
            bus: RefCell::new(None),
            state: Cell::new(AgentState::Off),
            owner: RefCell::new(String::new()),
            pending: RefCell::new(None),
            answered: Cell::new(false),
            registration: RefCell::new(None),
            on_event: RefCell::new(None),
            title,
            entry,
        });
        let me = Rc::downgrade(&inner);
        inner.entry.connect_activate(move |_| {
            if let Some(i) = me.upgrade() {
                i.submit();
            }
        });
        let me = Rc::downgrade(&inner);
        connect.connect_clicked(move |_| {
            if let Some(i) = me.upgrade() {
                i.submit();
            }
        });
        let me = Rc::downgrade(&inner);
        cancel.connect_clicked(move |_| {
            if let Some(i) = me.upgrade() {
                i.cancel_pending();
                i.emit(Event::Done);
            }
        });
        Agent { page, inner }
    }

    pub fn connect(&self, bus: Option<gio::DBusConnection>, on_event: impl Fn(Event) + 'static) {
        self.inner.bus.replace(bus);
        self.inner.on_event.replace(Some(Rc::new(on_event)));
    }

    /// Se uma senha foi dada desde o último pedido; zera a marca.
    pub fn take_answered(&self) -> bool {
        self.inner.answered.replace(false)
    }

    pub fn asking(&self) -> bool {
        self.inner.pending.borrow().is_some()
    }

    /// Mostra o pedido de senha desta rede (o nome vem de fora: [`shown_ssid`]).
    pub fn ask(&self, ssid: &str) {
        let ssid = shown_ssid(ssid);
        let title = if ssid.is_empty() { tr("Password") } else { tr("Password for %s").replace("%s", &ssid) };
        self.inner.title.set_text(&title);
        self.inner.entry.set_text("");
        self.inner.entry.grab_focus();
    }

    /// Cancela o pedido em aberto (Esc, fechar o popup, desconectar).
    pub fn cancel(&self) {
        if self.inner.cancel_pending() {
            self.inner.emit(Event::Done);
        }
    }

    /// O iwd trocou de dono (reiniciou ou saiu): o registro antigo não vale mais.
    pub fn reset(&self) {
        drop(self.inner.pending.borrow_mut().take());
        self.inner.entry.set_text("");
        self.inner.owner.replace(String::new());
        if self.inner.state.get() != AgentState::Off {
            self.inner.set_state(AgentState::Off);
        }
        self.inner.emit(Event::Done);
    }

    /// Registra o agente quando o popup mostra redes do iwd; sem elas (o NM voltou), tira o registro.
    pub fn want(&self, wanted: bool) {
        let state = self.inner.state.get();
        if wanted && state == AgentState::Off {
            self.register();
        } else if !wanted && state == AgentState::Registered {
            self.unregister();
        }
    }

    fn register(&self) {
        let Some(bus) = self.inner.bus.borrow().clone() else { return };
        if self.inner.registration.borrow().is_none() {
            let info = gio::DBusNodeInfo::for_xml(XML).ok().and_then(|node| node.lookup_interface(iwd::IWD_AGENT));
            let Some(info) = info else { return };
            let me: Weak<Inner> = Rc::downgrade(&self.inner);
            let registration = bus
                .register_object(PATH, &info)
                .method_call(move |_, sender, _, _, method, params, invocation| match me.upgrade() {
                    Some(i) => i.method_call(sender, method, &params, invocation),
                    None => invocation.return_dbus_error(CANCELED, "closing"),
                })
                .build();
            match registration {
                Ok(id) => {
                    self.inner.registration.replace(Some(id));
                }
                Err(err) => {
                    self.inner.state.set(AgentState::Failed);
                    self.inner.emit(Event::Failed(error_text(&err)));
                    return;
                }
            }
        }
        self.inner.set_state(AgentState::Registering);
        // Primeiro o dono do nome (para conferir quem chama), depois o registro
        let me = Rc::downgrade(&self.inner);
        let b = bus.clone();
        bus.call(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", "org.freedesktop.DBus", "GetNameOwner", Some(&(iwd::IWD,).to_variant()),
            Some(glib::VariantTy::new("(s)").expect("tipo")), gio::DBusCallFlags::NONE, CALL_MS, gio::Cancellable::NONE, move |result| {
                let Some(inner) = me.upgrade() else { return };
                let Some((owner,)) = result.ok().and_then(|v| v.get::<(String,)>()) else {
                    inner.state.set(AgentState::Off); // o iwd saiu: o próximo aviso tenta de novo
                    return;
                };
                inner.owner.replace(owner);
                let path = glib::variant::ObjectPath::try_from(PATH.to_string()).expect("caminho").to_variant();
                let me = Rc::downgrade(&inner);
                b.call(Some(iwd::IWD), iwd::IWD_AGENT_MANAGER_PATH, iwd::IWD_AGENT_MANAGER, "RegisterAgent", Some(&glib::Variant::tuple_from_iter([path])),
                    None, gio::DBusCallFlags::NO_AUTO_START, CALL_MS, gio::Cancellable::NONE, move |result| {
                        let Some(inner) = me.upgrade() else { return };
                        match result {
                            Ok(_) => inner.set_state(AgentState::Registered),
                            Err(err) => {
                                let remote = gio::DBusError::remote_error(&err).map(|n| n.to_string()).unwrap_or_default();
                                if remote == "net.connman.iwd.AlreadyExists" {
                                    inner.set_state(AgentState::Taken);
                                } else {
                                    inner.state.set(AgentState::Failed);
                                    inner.emit(Event::Failed(error_text(&err)));
                                }
                            }
                        }
                    });
            });
    }

    fn unregister(&self) {
        self.cancel();
        self.inner.state.set(AgentState::Off);
        let Some(bus) = self.inner.bus.borrow().clone() else { return };
        let path = glib::variant::ObjectPath::try_from(PATH.to_string()).expect("caminho").to_variant();
        bus.call(Some(iwd::IWD), iwd::IWD_AGENT_MANAGER_PATH, iwd::IWD_AGENT_MANAGER, "UnregisterAgent", Some(&glib::Variant::tuple_from_iter([path])),
            None, gio::DBusCallFlags::NO_AUTO_START, CALL_MS, gio::Cancellable::NONE, |_| {});
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ssid_shown_is_plain_and_short() {
        assert_eq!(shown_ssid("casa"), "casa");
        assert_eq!(shown_ssid("a\nb\u{1b}[31mc"), "ab[31mc");
        assert_eq!(shown_ssid(&"x".repeat(80)).chars().count(), 32);
        assert_eq!(shown_ssid("<b>&amp;</b>"), "<b>&amp;</b>", "o rótulo é texto puro: nada a escapar");
    }

    #[test]
    fn the_interface_parses() {
        let node = gio::DBusNodeInfo::for_xml(XML).unwrap();
        let iface = node.lookup_interface(iwd::IWD_AGENT).unwrap();
        for method in ["Release", "RequestPassphrase", "RequestPrivateKeyPassphrase", "RequestUserNameAndPassword", "RequestUserPassword", "Cancel"] {
            assert!(iface.lookup_method(method).is_some(), "{method}");
        }
    }
}
