# shellcheck shell=sh
# As traduções dos scripts, pelo gettext (domínio sway-desktop). Incluído com ".":
#   if [ -r "${0%/*}/sway-i18n.sh" ]; then . "${0%/*}/sway-i18n.sh"; else t() { printf '%s' "$1"; }; fi
# Uso: t 'Text in English'; com valores: printf "$(t 'Saved to %s')" "$file"
#      plural: printf "$(tn '%d package' '%d packages' "$n")" "$n"
#
# A língua é a do locale do config ou, sem ele, a do ambiente. O GNU gettext ignora o LANGUAGE
# com o locale C (também o C.UTF-8): nesse caso, vale qualquer locale UTF-8 instalado, só para
# ele ler o catálogo. Sem o comando gettext, o texto fica em inglês. Nada aqui pode falhar nem
# escrever no stderr: os scripts que o incluem rodam com set -e e com a saída num log.
_sway_i18n_self=$(readlink -f -- "$0" 2>/dev/null) || _sway_i18n_self=$0
_sway_i18n_dir=${SWAY_I18N_LOCALEDIR:-${_sway_i18n_self%/*}/../share/locale}
# Instalado por pacote (swaydesk-scripts), este arquivo fica em /usr/libexec/swaydesk, e as traduções em
# /usr/share/locale (swaydesk-common)
[ -d "$_sway_i18n_dir" ] || _sway_i18n_dir=/usr/share/locale
# A pasta velha (o link da migração, ou a que ficou) ou a swaydesk
_sway_i18n_conf=${XDG_CONFIG_HOME:-$HOME/.config}/sway-desktop
[ -e "$_sway_i18n_conf" ] || _sway_i18n_conf=${XDG_CONFIG_HOME:-$HOME/.config}/swaydesk
_sway_i18n_lang=$(sed -n 's/^[[:space:]]*locale[[:space:]]*=[[:space:]]*"\([a-z][a-z]_[A-Z][A-Z]\)".*/\1/p' \
  "$_sway_i18n_conf/config.toml" 2>/dev/null) || _sway_i18n_lang=
_sway_i18n_lang=${_sway_i18n_lang%%"
"*}
if [ -z "$_sway_i18n_lang" ]; then
  _sway_i18n_lang=${LANGUAGE:-${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}}
  _sway_i18n_lang=${_sway_i18n_lang%%.*}
  case $_sway_i18n_lang in C|POSIX) _sway_i18n_lang= ;; esac
fi
_sway_i18n_lc=
case ${LC_ALL:-${LC_MESSAGES:-${LANG:-C}}} in
  C|C.*|POSIX)
    if [ -n "$_sway_i18n_lang" ]; then
      for _sway_i18n_each in $(locale -a 2>/dev/null) ; do
        case $_sway_i18n_each in
          C.*|POSIX) ;;
          *.utf8|*.UTF-8|*.utf-8) _sway_i18n_lc=$_sway_i18n_each; break ;;
        esac
      done
    fi ;;
esac
command -v gettext >/dev/null 2>&1 || _sway_i18n_lang=

t() {
  if [ -z "$_sway_i18n_lang" ]; then
    printf '%s' "$1"
  elif [ -n "$_sway_i18n_lc" ]; then
    LC_ALL=$_sway_i18n_lc LANGUAGE=$_sway_i18n_lang TEXTDOMAINDIR=$_sway_i18n_dir \
      gettext -d sway-desktop -- "$1" 2>/dev/null || printf '%s' "$1"
  else
    LANGUAGE=$_sway_i18n_lang TEXTDOMAINDIR=$_sway_i18n_dir gettext -d sway-desktop -- "$1" 2>/dev/null \
      || printf '%s' "$1"
  fi
}

tn() {
  if [ -z "$_sway_i18n_lang" ]; then
    if [ "$3" = 1 ]; then printf '%s' "$1"; else printf '%s' "$2"; fi
  elif [ -n "$_sway_i18n_lc" ]; then
    LC_ALL=$_sway_i18n_lc LANGUAGE=$_sway_i18n_lang TEXTDOMAINDIR=$_sway_i18n_dir \
      ngettext -d sway-desktop -- "$1" "$2" "$3" 2>/dev/null || printf '%s' "$2"
  else
    LANGUAGE=$_sway_i18n_lang TEXTDOMAINDIR=$_sway_i18n_dir ngettext -d sway-desktop -- "$1" "$2" "$3" 2>/dev/null \
      || printf '%s' "$2"
  fi
}
