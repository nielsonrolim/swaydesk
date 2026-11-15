//! A lógica do seletor de janelas (sway-switcher), sem GTK: a ordem de uso e a seleção do Alt+Tab. O porte
//! do `sway_switcher.py`; os testes de contrato da suíte Python rodam contra as duas versões.
//!
//! O residente guarda a ordem de uso pelos ids do Sway (con_id), atualizada pelos eventos "window" do IPC.
//! Os títulos, os apps e os workspaces vêm da árvore na hora de abrir: a ordem só decide quem vem antes.
use serde_json::Value;
pub use swaydesk_dock_logic::{windows_from_tree, Window};

pub const TITLE_MAX: usize = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Alt+Tab: a janela usada antes; de novo, a seguinte
    Next,
    /// Alt+Shift+Tab: a mais antiga; de novo, a anterior
    Prev,
}

/// A ordem de uso: o primeiro é o foco atual, o segundo a janela usada antes, e assim por diante.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Order {
    pub ids: Vec<i64>,
}

impl Order {
    pub fn new(ids: impl IntoIterator<Item = i64>) -> Order {
        let mut order = Order::default();
        for id in ids {
            order.add(id);
        }
        order
    }

    /// Na subida: o foco atual primeiro, as outras na ordem de criação.
    pub fn from_tree(tree: &Value) -> Order {
        let windows = windows_from_tree(tree);
        let focused = windows.iter().filter(|w| w.focused).map(|w| w.id);
        Order::new(focused.chain(windows.iter().filter(|w| !w.focused).map(|w| w.id)).collect::<Vec<_>>())
    }

    pub fn focus(&mut self, id: i64) {
        self.ids.retain(|&x| x != id);
        self.ids.insert(0, id);
    }

    pub fn close(&mut self, id: i64) {
        self.ids.retain(|&x| x != id);
    }

    pub fn add(&mut self, id: i64) {
        if !self.ids.contains(&id) {
            self.ids.push(id);
        }
    }

    /// Um evento "window" do Sway. Devolve se a ordem mudou.
    pub fn apply_event(&mut self, event: &Value) -> bool {
        let Some(id) = event.get("container").and_then(|c| c.get("id")).and_then(Value::as_i64) else { return false };
        let before = self.ids.clone();
        match event.get("change").and_then(Value::as_str) {
            Some("focus") => self.focus(id),
            Some("close") => self.close(id),
            Some("new") => self.add(id),
            _ => {}
        }
        self.ids != before
    }

    /// As janelas abertas da árvore, na ordem de uso. As que a ordem não conhece vão para o fim, na ordem
    /// de criação; as que fecharam sem o evento somem.
    pub fn windows(&self, tree: &Value) -> Vec<Window> {
        let mut windows = windows_from_tree(tree);
        let rank = |id: i64| self.ids.iter().position(|&x| x == id).unwrap_or(self.ids.len());
        windows.sort_by_key(|w| (rank(w.id), w.id));
        windows
    }
}

/// Junta as linhas do `swaymsg -m` em eventos: ele escreve cada evento como JSON formatado, em várias
/// linhas, ou numa linha só. A cada linha, tenta ler o que juntou; um evento inválido some quando chega a
/// linha que o fecharia (um "}" sozinho) ou passa de MAX_EVENT, para a leitura não travar.
#[derive(Default)]
pub struct EventReader {
    text: String,
}

impl EventReader {
    pub const MAX_EVENT: usize = 1 << 20;

    /// O evento completo com esta linha, ou None.
    pub fn feed(&mut self, line: &str) -> Option<Value> {
        self.text.push_str(line);
        match serde_json::from_str::<Value>(&self.text) {
            Ok(event) => {
                self.text.clear();
                Some(event)
            }
            Err(_) => {
                if line.trim_end() == "}" || self.text.len() > Self::MAX_EVENT {
                    self.text.clear();
                }
                None
            }
        }
    }
}

/// O item selecionado ao abrir: o next vai à janela usada antes (a segunda), o prev à mais antiga.
pub fn first_selection(count: usize, command: Command) -> usize {
    match (count, command) {
        (0 | 1, _) => 0,
        (_, Command::Next) => 1,
        (_, Command::Prev) => count - 1,
    }
}

/// O next e o prev com a janela aberta: dão a volta nas pontas.
pub fn step(selected: usize, count: usize, command: Command) -> usize {
    if count == 0 {
        return 0;
    }
    match command {
        Command::Next => (selected + 1) % count,
        Command::Prev => (selected + count - 1) % count,
    }
}

/// Um Alt+Tab que chega antes de o cartão receber o teclado é um toque novo, não um Tab com o Alt segurado:
/// o toque anterior ainda não foi confirmado porque a soltura do Alt só se vê com o teclado. Dois toques
/// seguidos voltam à janela de onde se saiu (a primeira da lista), três vão de novo à usada antes.
pub fn retap(selected: usize, count: usize, command: Command) -> usize {
    let first = first_selection(count, command);
    if selected == first { 0 } else { first }
}

/// Sem o residente não há ordem de uso: o next vai à janela seguinte na árvore e o prev à anterior, dando
/// a volta. None se não houver outra.
pub fn without_daemon_target(tree: &Value, command: Command) -> Option<i64> {
    let windows = windows_from_tree(tree);
    if windows.len() < 2 {
        return None;
    }
    let current = windows.iter().position(|w| w.focused).unwrap_or(0);
    Some(windows[step(current, windows.len(), command)].id)
}

/// O título numa linha e encurtado com reticências.
pub fn short_title(title: &str) -> String {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.chars().count() <= TITLE_MAX {
        return title;
    }
    let cut: String = title.chars().take(TITLE_MAX - 1).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn con(id: i64, app: &str, name: &str, focused: bool) -> Value {
        json!({"type": "con", "id": id, "pid": 100, "app_id": app, "name": name, "focused": focused,
               "nodes": [], "floating_nodes": []})
    }

    /// O mesmo da tests/test_switcher.py.
    fn tree() -> Value {
        json!({"type": "root", "id": 1, "nodes": [
            {"type": "output", "name": "__i3", "id": 2, "nodes": [
                {"type": "workspace", "name": "__i3_scratch", "id": 3, "nodes": [], "floating_nodes": [con(99, "hidden", "", false)]}]},
            {"type": "output", "name": "HDMI-A-1", "id": 4, "nodes": [
                {"type": "workspace", "name": "1", "id": 10, "nodes": [con(20, "kitty", "~", false), con(21, "firefox", "Site", true)], "floating_nodes": []},
                {"type": "workspace", "name": "2", "id": 11, "nodes": [con(22, "org.gnome.Nautilus", "Pasta", false)], "floating_nodes": []}]}]})
    }

    #[test]
    fn starts_with_the_focused_window() {
        assert_eq!(Order::from_tree(&tree()).ids, vec![21, 20, 22]);
    }

    #[test]
    fn events_change_the_order() {
        let mut order = Order::new([21, 20, 22]);
        assert!(order.apply_event(&json!({"change": "focus", "container": {"id": 22}})));
        assert_eq!(order.ids, vec![22, 21, 20]);
        assert!(order.apply_event(&json!({"change": "close", "container": {"id": 21}})));
        assert_eq!(order.ids, vec![22, 20]);
        assert!(order.apply_event(&json!({"change": "new", "container": {"id": 30}})));
        assert_eq!(order.ids, vec![22, 20, 30]);
        assert!(!order.apply_event(&json!({"change": "title", "container": {"id": 20}})));
        for bad in [json!({}), json!({"change": "focus"}), json!({"change": "focus", "container": {"id": "x"}}),
                    json!({"change": "focus", "container": {"id": true}}), json!([]), json!("focus")] {
            assert!(!order.apply_event(&bad), "{bad}");
        }
        assert_eq!(order.ids, vec![22, 20, 30]);
    }

    #[test]
    fn windows_in_use_order() {
        let mut order = Order::new([22, 21]);
        order.add(30);
        let windows = order.windows(&tree());
        assert_eq!(windows.iter().map(|w| w.id).collect::<Vec<_>>(), vec![22, 21, 20]);
        assert_eq!((windows[0].app_id.as_str(), windows[0].title.as_str(), windows[0].workspace.as_str()), ("org.gnome.Nautilus", "Pasta", "2"));
    }

    #[test]
    fn selection() {
        assert_eq!(first_selection(3, Command::Next), 1);
        assert_eq!(first_selection(3, Command::Prev), 2);
        assert_eq!(first_selection(1, Command::Next), 0);
        assert_eq!(first_selection(0, Command::Prev), 0);
        assert_eq!(step(2, 3, Command::Next), 0);
        assert_eq!(step(0, 3, Command::Prev), 2);
        assert_eq!(step(1, 3, Command::Next), 2);
        assert_eq!(step(0, 0, Command::Next), 0);
        assert_eq!(retap(1, 3, Command::Next), 0);
        assert_eq!(retap(0, 3, Command::Next), 1);
        assert_eq!(retap(2, 3, Command::Prev), 0);
        assert_eq!(retap(0, 3, Command::Prev), 2);
        assert_eq!(without_daemon_target(&tree(), Command::Next), Some(22));
        assert_eq!(without_daemon_target(&tree(), Command::Prev), Some(20));
    }

    #[test]
    fn titles() {
        assert_eq!(short_title("a\n  b"), "a b");
        let long = short_title(&"x".repeat(100));
        assert_eq!(long.chars().count(), TITLE_MAX);
        assert!(long.ends_with('…'));
        assert_eq!(short_title(&"é".repeat(TITLE_MAX)).chars().count(), TITLE_MAX);
    }

    #[test]
    fn event_reader() {
        let mut reader = EventReader::default();
        let pretty = serde_json::to_string_pretty(&json!({"change": "focus", "container": {"id": 7}})).unwrap() + "\n";
        let lines: Vec<String> = pretty.split_inclusive('\n').map(str::to_string).collect();
        let got: Vec<Option<Value>> = lines.iter().map(|l| reader.feed(l)).collect();
        assert!(got[..got.len() - 1].iter().all(Option::is_none));
        assert_eq!(got.last().unwrap().as_ref().unwrap()["container"]["id"], 7);
        assert_eq!(reader.feed("{\"change\": \"close\", \"container\": {\"id\": 8}}\n").unwrap()["change"], "close");
        for line in ["{\n", "  \"change\": oops\n", "}\n"] {
            assert!(reader.feed(line).is_none());
        }
        assert_eq!(reader.feed("{\"change\": \"new\", \"container\": {\"id\": 9}}\n").unwrap()["container"]["id"], 9);
    }
}
