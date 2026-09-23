# FFM — português do Brasil.
#
# Stub: every id from en.ftl is present with the same placeables, which is what
# build.rs checks. Values are being translated incrementally — an id still
# carrying its English text is visibly untranslated in the sandbox's
# side-by-side view (/__sandbox/rows?lang=both) rather than silently missing.
#
# Household sample data — "Água · Sabesp", "Mesada" — is NOT app copy and must
# never appear in this file. It is content the household typed.

## Pré-login

login-title = Entrar
login-name = Nome
login-password = Senha
login-submit = Entrar
login-error-mismatch = Nome e senha não conferem.

setup-title = Configure sua casa
setup-household-section = Sua casa
setup-you-section = Você
setup-storage-section = Definido pelo operador
setup-household-name = Nome da casa
setup-currency = Moeda
setup-language = Idioma padrão
setup-password-confirm = Confirme a senha
setup-submit = Criar casa
setup-taken-title = Esta casa já foi configurada
setup-taken-body = Alguém já concluiu a configuração. Entre em vez disso.

invite-title = Entrar em { $household }
invite-name-taken = Esse nome já está em uso — escolha outro.
invite-dead-title = Este link expirou
invite-dead-body = Peça um novo para um administrador.

## Painel

ledger-ins = ENTRADAS
ledger-outs = SAÍDAS
ledger-balance = Saldo
ledger-household-in = Entradas da casa
ledger-household-out = Saídas
ledger-household-net = Líquido

## Estados da linha

row-overdue-since = vencido desde { $date }
row-scheduled-for = agendado · vence { $date }
row-due = vence { $date }
row-paid-on = pago em { $date }
row-received-on = recebido em { $date }
row-manual-paid = marcado como pago
row-deleted = excluído
row-unsaved = não salvo · e para retomar
row-try-again = Tentar de novo

## Arquivos

file-bill = Conta
file-invoice = Fatura
file-scheduled = Agendado
file-receipt = Comprovante
file-payslip = Holerite
file-page = página { $page }
file-none-bill = nenhuma conta neste lançamento
file-none-scheduled = nenhum agendamento neste lançamento
file-none-receipt = nenhum comprovante neste lançamento
file-none-income-scheduled = agendamento não se aplica a uma entrada

## Recusas na statusline

sort-refuses-reorder = ledger.sort = "{ $sort }" — reordenar exige sort = "manual"
group-refuses-reorder = ledger.group = "{ $group }" — reordenar exige group = "none"
not-a-command = não é um comando: { $name } (tente help)
no-rule-behind-row = não foi gerado por uma regra recorrente

rows-moved =
    { $n ->
        [one] 1 linha movida { $dir }
       *[other] { $n } linhas movidas { $dir }
    }

## Pré-login

auth-brand-sub = livro de contas da casa
auth-where-login = entrar
auth-where-setup = primeira vez
auth-where-invite = convite
auth-lang-switch = Idioma
auth-theme-system = Sistema
auth-theme-light = Claro
auth-theme-dark = Escuro
auth-to-sign-in = Ir para entrar

login-subtitle = Entre no livro de contas da casa.
login-error-body = Confira o nome, ou peça um novo convite a um administrador.

setup-step = 1 / 1
setup-subtitle = Ninguém tem conta ainda, então esta primeira é a sua — e é a de administrador.
setup-household-hint = Aparece na página de entrada.
setup-note-axes = As datas são sempre { $format }. O dinheiro segue a moeda escolhida. O idioma muda só as palavras.
setup-aside-signin = você vai entrar com isto
setup-theme = Tema de cores
setup-theme-hint = Dá para trocar quando quiser em :profile.
setup-line-numbers = Números de linha
setup-nu-relative = Relativos
setup-nu-absolute = Absolutos
setup-nu-none = Nenhum
setup-storage-hint = Edite o ffm.toml e reinicie para mudar isto.
setup-done-title = Já configurado
setup-done-body = Esta casa já tem membros, então não há nada a configurar. Entre, ou peça um convite a um administrador.

currency-brl = Real
currency-usd = Dólar americano
currency-eur = Euro
currency-gbp = Libra

invite-subtitle = { $admin } convidou você. Escolha seu próprio nome e senha — ninguém mais viu este link.
invite-name-hint = Esta também é a sua pasta no cofre de comprovantes, então é mais fácil acertar agora.
invite-language = Idioma
invite-language-hint = Datas e valores não mudam com isto.
invite-submit = Entrar na casa
invite-expiry = Este link funciona uma vez e expira em { $date }.
invite-used-title = Este convite já foi usado.
invite-used-body = Se não foi você, peça a um administrador para revogá-lo.
invite-expired-title = Este convite expirou em { $date }.
invite-expired-body = Peça um link novo a um administrador.
invite-unknown-title = Este link não funciona.
invite-unknown-body = Confira se copiou o link inteiro, ou peça um novo a um administrador.

reset-title = Defina uma nova senha
reset-subtitle = Sua conta continua a mesma — muda só a senha.
reset-submit = Definir a senha

field-required = { $field } não pode ficar vazio.
password-too-short = Use pelo menos { $n } caracteres.
password-mismatch = As duas senhas não são iguais.
name-taken = Alguém já usa esse nome.
name-taken-body = Escolha outro — o nome é como cada um entra, então precisa ser único.

sign-out = Sair
signed-out = Sessão encerrada.

## Alterações

row-title = Descrição
row-amount = Valor
row-add-spend = Adicionar gasto
row-add-income = Adicionar entrada
row-save = Salvar
row-cancel = Cancelar
row-added = adicionado "{ $title }"
row-deleted-say = excluído · u para desfazer
row-delete-confirm = Excluir esta linha?
row-delete-body = Ela continua no banco e `u` traz de volta. Aperte y para confirmar.
row-delete-yes = Excluir
nothing-to-undo = nada para desfazer
undone = desfeito
error-not-written = Não deu certo — nada foi salvo.

## Regras recorrentes

rule-new-spend = Nova conta recorrente
rule-new-income = Nova entrada recorrente
rule-edit = Regra recorrente
rule-bill-name = Nome da conta
rule-source = Origem
rule-bucket = Categoria
rule-cadence = Frequência
rule-start = Início
rule-due-day = Vencimento
rule-expected-day = Dia previsto
rule-end = Fim
rule-installments = Parcelas
rule-payments = Pagamentos
rule-tracked = Guardar comprovante no disco
rule-no-barcode = Esta conta não tem código de barras
rule-carry-forward = Repetir o valor do mês anterior
rule-varies = O valor varia
rule-estimate-basis = Estimar pelo
rule-estimate-lowest = Menor
rule-estimate-average = Média
rule-estimate-last = Último
rule-estimate-window = dos últimos { $n } confirmados
rule-instance = Identificação
rule-private = Escondido dos outros membros
rule-amount-in-filename = Valor no nome do arquivo
rule-create = Criar
rule-save = Salvar
rule-delete = Excluir regra
rule-path-legend = categoria / conta / identificação / ano / mês.ext
rule-past-note = Os meses passados ficam como foram registrados. Isto vale para os próximos.
rule-created = regra criada · { $n } mês(es) gerado(s)
rule-saved = regra salva
rule-deleted = regra excluída · as linhas continuam
rule-none-on-row = não vem de uma regra recorrente
cadence-monthly = Mensal
cadence-bimonthly = A cada 2 meses
cadence-quarterly = A cada 3 meses
cadence-four = A cada 4 meses
cadence-half = A cada 6 meses
cadence-yearly = Anual

## Arquivos

file-add-bill = Anexar conta
file-add-scheduled = Anexar agendamento
file-add-receipt = Anexar comprovante
file-added = arquivo anexado
file-removed = arquivo movido para .delete · u para trazer de volta
file-open = Abrir em nova aba
file-delete = Excluir arquivo
file-not-tracked = esta linha não tem onde arquivar
file-too-big = esse arquivo é grande demais para guardar
file-preview = Arquivos
file-no-files = nenhum arquivo nesta linha
file-page-of = Um arquivo, { $total } páginas — este mês é a página { $page }
file-page-outside = página { $page } — fora das { $total } páginas deste arquivo, é preciso enviar um novo para este mês
rule-spans-period = O código de barras cobre vários meses
rule-window-size = Páginas neste arquivo
rule-window-start = A primeira página é de
estimate-no-history = nenhum mês confirmado ainda — usa o valor da própria regra

## Configurações

settings-profile = Suas preferências
settings-household = Casa
settings-members = Membros
settings-saved = { $name } salvo
settings-admin-only = isso é só para administradores
pane-close = esc

profile-appearance = Aparência
profile-ledger = Livro
profile-layout = Layout
profile-privacy = Linhas novas
profile-theme = Tema de cores
profile-line-numbers = Números de linha
profile-panes = Painéis
profile-min-split = Abaixo desta largura, usar abas
profile-max-width = Largura máxima do painel
profile-gap = Espaço entre painéis
profile-sort = Ordem das linhas
profile-sort-manual = Manual
profile-sort-date = Data
profile-sort-amount = Valor
profile-search-scope = Onde buscar
profile-scope-pane = Neste painel
profile-scope-household = Na casa toda
profile-show-delta = Mostrar variação em relação ao mês anterior
profile-fold-default = Grupos começam
profile-fold-expanded = Abertos
profile-fold-collapsed = Fechados
profile-visibility-inherit = Seguir a casa ({ $resolved })
profile-visibility-household = Compartilhado com a casa
profile-visibility-private = Só meu
profile-devices = Conectado em
profile-no-devices = nenhum outro aparelho

household-name = Nome da casa
household-currency = Moeda
household-language = Idioma padrão
household-visibility = Padrão para linhas novas
household-show-recurring = Usar regras recorrentes
household-storage = Definido por quem roda o programa
household-storage-note = Edite o ffm.toml e reinicie para mudar isto.

members-role-admin = Administrador
members-role-member = Membro
members-invite-label = Para quem (opcional)
members-invite-create = Criar link de convite
members-invite-note = Você nunca digita a senha de outra pessoa. Ela define na hora de aceitar o link, junto com o nome.
members-link-once = Este link aparece uma vez só. Crie outro se perder.
members-copy = Copiar
members-pending = Links pendentes
members-expires = expira em { $date }
members-expired = expirado
members-revoke = Revogar
members-reset = Redefinir senha
members-signout = Desconectar de tudo
members-reset-note = Redefinir a senha desconecta { $name } de tudo e fica registrado no histórico.
members-signed-out = { $name } foi desconectado de { $n } aparelho(s)
members-no-sessions = não está conectado em lugar nenhum

command-placeholder = comando
members-self-note = Este é você. Troque uma senha que você sabe em :profile; recupere uma que esqueceu com `ffm reset-link` no console.

## Histórico — `gl`

log-title = Histórico
log-empty = Nada foi registrado nesta linha ainda.
log-today = hoje
log-yesterday = ontem
log-filter-row = esta linha
log-filter-rule = regra
log-filter-all = tudo
log-rule-tag = regra
log-undo-of = desfaz o de { $time }
log-fields = { $n ->
        [one] 1 campo
       *[other] { $n } campos
    }
log-system = sistema
log-materializer = gerador
log-import = importação

log-action-create = adicionou
log-action-edit = editou
log-action-delete = excluiu
log-action-restore = restaurou
log-action-reattribute = passou para outro
log-action-reorder = moveu
log-action-privacy = mudou a privacidade
log-action-rule-create = criou a regra
log-action-rule-edit = editou a regra
log-action-file-attach = anexou o arquivo
log-action-file-unlink = desanexou o arquivo
log-action-file-upload = anexou o arquivo
log-action-file-delete = removeu o arquivo
log-action-import = importou
log-action-materialize = gerou
log-action-password-reset = redefiniu a senha
log-action-sessions-revoked = desconectou de tudo

log-field-amount = valor
log-field-title = descrição
log-field-date = data
log-field-due-day = dia de vencimento
log-field-bucket = categoria
log-field-bill-name = nome da conta
log-field-instance = identificação
log-field-cadence = frequência
log-field-start-date = início
log-field-end-date = fim
log-field-tracked = guardar comprovante
log-field-path = caminho
log-field-kind = tipo
log-field-original-filename = nome original
log-on = ligado
log-off = desligado
moved-down = para baixo
moved-up = para cima
group-uncategorized = Sem categoria
group-rows = { $n ->
        [one] 1 linha
       *[other] { $n } linhas
    }
grep-title = Encontrado: { $term }
grep-none = nada corresponde a isso, em nenhum mês
grep-count = { $n ->
        [one] 1 linha
       *[other] { $n } linhas
    }

## Importação de contas

import-dropzone = Solte a conta aqui, ou escolha um arquivo
import-client-code = Código do cliente
import-unlink = Desvincular
import-read = li { $what } da conta
import-filled = li { $amount } com vencimento { $date } do código de barras
import-amount-differs = o código de barras diz { $barcode }, esta linha diz { $row } — mantida como estava
import-date-differs = o código de barras diz { $barcode }, esta linha diz { $row } — mantida como estava
import-code-differs = o código do cliente desta conta é { $found }, mas a regra tem { $known } — mantido como estava
import-unresolved = o código de barras não trazia { $what }
import-no-occurrence = não há linha deste mês para anexar
import-unlinked = código do cliente desvinculado
import-already-attached = esta conta já está em { $period } — uma conta, uma linha

## Seletor de mês

months-short = Jan Fev Mar Abr Mai Jun Jul Ago Set Out Nov Dez
month-picker = Mês
unresolved-dot = { $n ->
        [one] 1 conta vencida sem comprovante
       *[other] { $n } contas vencidas sem comprovante
    }
unresolved-here = vencidas aqui

## `e` — editar a linha

edit-day = Dia
edit-private = Escondido dos outros membros
edit-saved = salvo
row-reattributed = passou para { $name }
row-now-private = escondido dos outros
row-now-shared = compartilhado com a casa
reattribute-private-title = Passar uma linha escondida para { $name }?
reattribute-private-body = A linha passa a ser dela, então você deixa de vê-la. Aperte y para confirmar.
no-pane-that-way = não há painel desse lado
drafts-parked = { $n ->
        [one] 1 rascunho não salvo
       *[other] { $n } rascunhos não salvos
    }

## `gd` e o carrossel `P`

details-due = Vence
details-cadence = Frequência
details-receipt = Comprovante
details-status = Situação
details-none = nada recorrente por trás desta linha
status-paid = comprovante arquivado
status-scheduled = agendado, ainda não venceu
status-scheduled-overdue = agendado, já venceu e sem comprovante
status-pending = nada agendado e nada pago ainda
status-plain = sem acompanhamento, então não há prova nem de um lado nem do outro
status-manual-paid = marcado à mão — sem acompanhamento, nada comprova
status-expected = ainda não chegou
status-received = chegou

carousel-open-tab = Abrir em nova aba
carousel-zoom-reset = Zerar o zoom
keymap-title = Atalhos
keymap-close = Fechar
details-months = mês(es)
keymap-move = Mover
keymap-act = Agir
keymap-find = Buscar
keymap-fold = Dobrar
keymap-files = Arquivos
keymap-undo = Desfazer
keymap-type = Digitar

## Meses sazonais — em que meses do ano uma regra realmente acontece.
## A expressão em si é numérica em qualquer idioma; só estas palavras mudam.
months-seasonal = Só alguns meses
months-seasonal-hint = uma conta que vale parte do ano, todo ano
months-label = Meses
months-placeholder = 1-10
months-help = 1-10 intervalo · 1,3,5 lista · ^11,^12 todos exceto
months-every = todo mês
months-count-skipped = { $n ->
    [one] um mês por ano
   *[other] { $n } meses por ano
  } · { $skipped } de fora
months-err-empty = isso ainda não é um mês
months-err-not-a-number = não é número de mês: { $token }
months-err-out-of-range = mês { $n } — os meses vão de 1 a 12
months-err-backwards = { $range } está invertido
months-err-nothing-left = assim não sobra nenhum mês
month-short = { $n ->
    [1] jan
    [2] fev
    [3] mar
    [4] abr
    [5] mai
    [6] jun
    [7] jul
    [8] ago
    [9] set
   [10] out
   [11] nov
   *[12] dez
  }

## Um salvamento recusado porque a linha mudou embaixo do formulário. Nada foi
## gravado, e o que foi digitado continua guardado na linha.
edit-stale = { $who } mudou esta linha enquanto você editava. Nada foi gravado — sua edição continua aqui, aperte e para retomar.
edit-stale-unknown = Esta linha mudou enquanto você editava. Nada foi gravado — sua edição continua aqui, aperte e para retomar.
edit-gone = Esta linha foi excluída enquanto você editava. Nada foi gravado — aperte u para trazer de volta.
edit-day-bad = Um dia do mês vai de 1 a 31.
grep-needs-a-term = grep precisa de algo para buscar
cursor-on-fold = isso é um grupo, não uma linha
