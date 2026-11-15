//! O renderizador do GTK 4 dos componentes: o cairo, a não ser que o ambiente já escolha outro
//! (GSK_RENDERER). Nas VMs, o padrão (o NGL, por GL) dobrou o RSS de um popup e somou uns 23 MB de memória
//! suja, de mapeamentos e buffers do driver; na grade e no notifd, ~25 MB cada. As janelas do projeto são
//! cartões parados, sem animação: o desenho por CPU não faz falta. Os programas que um componente abre não
//! herdam o ajuste: quem abre apps tira a variável com o [`clean_launch_context`], o
//! [`clean_subprocess_launcher`] ou o [`clean_command`].
use gio::prelude::*;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

/// Se foi este processo que definiu o GSK_RENDERER (pelo [`use_cairo_renderer`]). Vindo do ambiente, a
/// variável passa adiante aos programas abertos, como antes.
static SET_HERE: AtomicBool = AtomicBool::new(false);

/// Escolhe o cairo se o ambiente não traz um renderizador. Chame antes de criar linhas (o `set_var` não é
/// seguro com outras linhas lendo o ambiente) e antes do gtk4::init. Num programa que se reexecuta com
/// outro nome, chame depois do reexec: o processo novo herdaria a variável e a tomaria como vinda do
/// ambiente.
pub fn use_cairo_renderer() {
    if std::env::var_os("GSK_RENDERER").is_none() {
        std::env::set_var("GSK_RENDERER", "cairo");
        SET_HERE.store(true, Ordering::SeqCst);
    }
}

/// Se foi este processo que definiu o GSK_RENDERER.
pub fn renderer_set_here() -> bool {
    SET_HERE.load(Ordering::SeqCst)
}

/// Só para os testes dos crates que dependem deste: marca (ou desmarca) a variável como definida aqui.
#[doc(hidden)]
pub fn mark_set_here_for_tests(set: bool) {
    SET_HERE.store(set, Ordering::SeqCst);
}

/// O contexto de lançamento de apps do Gio (o `AppInfo::launch`) sem o GSK_RENDERER deste processo.
pub fn clean_launch_context(context: &impl IsA<gio::AppLaunchContext>) {
    if renderer_set_here() {
        context.unsetenv("GSK_RENDERER");
    }
}

/// O `SubprocessLauncher` do Gio sem o GSK_RENDERER deste processo.
pub fn clean_subprocess_launcher(launcher: &gio::SubprocessLauncher) {
    if renderer_set_here() {
        launcher.unsetenv("GSK_RENDERER");
    }
}

/// Um `Command` sem o GSK_RENDERER deste processo. Um app GTK 4 aberto (o pavucontrol, o Resources)
/// desenharia por CPU, com gráficos e animações mais lentos.
pub fn clean_command(command: &mut Command) -> &mut Command {
    if renderer_set_here() {
        command.env_remove("GSK_RENDERER");
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    // Um teste só: mexe no ambiente do processo, e os testes rodam em linhas paralelas
    #[test]
    fn cairo_only_when_the_environment_has_no_renderer() {
        std::env::set_var("GSK_RENDERER", "ngl");
        use_cairo_renderer();
        assert_eq!(std::env::var("GSK_RENDERER").as_deref(), Ok("ngl"));
        assert!(!renderer_set_here());
        let mut command = Command::new("true");
        clean_command(&mut command);
        assert!(command.get_envs().next().is_none(), "vinda do ambiente, a variável passa adiante");

        std::env::remove_var("GSK_RENDERER");
        use_cairo_renderer();
        assert_eq!(std::env::var("GSK_RENDERER").as_deref(), Ok("cairo"));
        assert!(renderer_set_here());
        let mut command = Command::new("true");
        clean_command(&mut command);
        let envs: Vec<_> = command.get_envs().collect();
        assert_eq!(envs, [(std::ffi::OsStr::new("GSK_RENDERER"), None)]);

        let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::NONE);
        clean_subprocess_launcher(&launcher);
        assert_eq!(launcher.getenv("GSK_RENDERER"), None);

        std::env::remove_var("GSK_RENDERER");
        mark_set_here_for_tests(false);
    }
}
