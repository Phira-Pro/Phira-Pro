
multiplayer = Multiplayer

connect = Conectar
connect-must-login = Você deve fazer login para entrar no modo multijogador
connect-success = Conectado com sucesso
connect-failed = Falhou ao conectar
connect-authenticate-failed = Falha na autorização
reconnect = Reconectando…

create-room = Criar sala
create-room-success = Sala criada
create-room-failed = Falha ao criar sala
create-invalid-id = O ID da sala consiste em no máximo 20 caracteres, incluindo letras, números, - (dash) e _ (underscore)

join-room = Juntar-se à sala
join-room-invalid-id = ID de  sala inválido
join-room-failed = Falha ao entrar na sala

leave-room = Sair da sala
leave-room-failed = Falha ao sair da sala

disconnect = desconectar

request-start = Começar o jogo
request-start-no-chart = Você não selecionou um gráfico
request-start-failed = Falha ao iniciar o jogo

user-list = Usuários

lock-room = { $current ->
  [true] Desbloquear sala
  *[other] bloquear sala
}
cycle-room = { $current ->
  [true] Modo de ciclismo
  *[other] Modo normal
}

ready = Preparar
ready-failed = Falha ao se preparar

cancel-ready = Cancelar

room-id = ID da sala: { $id }

download-failed = Falha ao baixar o gráfico

lock-room-failed = Falha ao bloquear a sala
cycle-room-failed = Falha ao alterar o modo de sala

chat-placeholder = Dizer alguma coisa…
chat-send = Send
chat-empty = A mensagem está vazia
chat-sent = Enviar
chat-send-failed = Falha ao enviar mensagem

select-chart-host-only = Somente o anfitrião pode selecionar o gráfico
select-chart-local = Não é possível selecionar o gráfico local
select-chart-failed = Falha ao selecionar o gráfico
select-chart-not-now = Você não pode selecionar o gráfico agora

msg-create-room = `{ $user }` criou a sala
msg-join-room = `{ $user }` entrou na sala
msg-leave-room = `{ $user }` deixou a sala
msg-new-host = `{ $user }` tornou-se o novo anfitrião
msg-select-chart = O anfitrião `{ $user }` selecionou gráfico  `{ $chart }` (#{ $id })
msg-game-start = O anfitrião `{ $user }` começou o jogo. Outros jogadores devem se preparar .
msg-ready = `{ $user }` está preparado
msg-cancel-ready = `{ $user }` cancelou
msg-cancel-game = `{ $user }` cancelou o jogo
msg-start-playing = jogo iniciado
msg-played = `{ $user }` terminou de jogar : { $score } ({ $accuracy }){ $full-combo ->
  [true] , combo completo
  *[other] {""}
}
msg-game-end = O jogo terminou
msg-abort = `{ $user }` abortou o jogo
msg-room-lock = { $lock ->
  [true] Sala bloqueada
  *[other] Sala desbloqueada
}
msg-room-cycle = { $cycle ->
  [true] Sala alterada para modo de ciclismo
  *[other] Sala alterada para modo normal
}

# ---- Phira Pro: local multiplayer ----
local-mp-host = Local Multiplayer
local-mp-rooms = Room List
local-mp-only-public = Public only
local-mp-all = All
local-mp-started = Local multiplayer started
local-mp-already = Local multiplayer is already running
local-mp-start-failed = Failed to start local multiplayer
local-mp-rooms-failed = Failed to fetch room list
local-mp-rooms-empty = No rooms
local-mp-public = Public
local-mp-private = Private
local-mp-state-select = Selecting chart
local-mp-state-ready = Waiting for ready
local-mp-state-playing = Playing
room-play-disabled = Only the room host can start the game.

local-mp-servers = Server List
local-mp-servers-loading = Fetching server list...
local-mp-servers-empty = No servers found
local-mp-servers-failed = Failed to fetch server list
local-mp-servers-offline = offline

room-password = Senha da sala
room-password-prompt = Senha da sala (deixe em branco se não houver)
local-mp-password = Senha
