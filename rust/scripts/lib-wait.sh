# As esperas dos testes headless (rust/scripts/headless-*.sh), com ". /repo/rust/scripts/lib-wait.sh".
# Num host carregado (um RC compilando ao lado), um app leva segundos para subir, desenhar ou sair, e um prazo fixo
# de 2 ou 5 s falhava só sob carga. Cada espera sai assim que a condição vale: o prazo só pesa num caso que falha.
# HEADLESS_DEADLINE muda o prazo (em segundos); $ticks é o mesmo prazo em voltas de 0,1 s, para os laços
# "for _ in $(seq $ticks); do COND && break; sleep 0.1; done".
deadline=${HEADLESS_DEADLINE:-30}
# Um número inteiro positivo: 0 faria cada espera falhar na hora, e um texto daria um erro de aritmética
[[ $deadline =~ ^[1-9][0-9]*$ ]] || { echo "HEADLESS_DEADLINE must be a positive integer (seconds): '$deadline'" >&2; exit 2; }
ticks=$((deadline * 10))

wait_for() {  # wait_for CMD...: roda CMD de 0,1 em 0,1 s até dar certo, por até $deadline s
  local n=$ticks
  while ((n-- > 0)); do "$@" && return 0; sleep 0.1; done
  return 1
}

wait_sway() {  # o socket do Sway headless; sem ele, nenhum caso roda: falha com o nome do passo e sai
  wait_for test -S "$XDG_RUNTIME_DIR/wayland-1" && return 0
  printf 'FALHOU  o Sway não subiu em %s s\n' "$deadline"
  exit 1
}
