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
login-signed-out = Este aparelho foi desconectado — por outro aparelho ou por um admin. Entre de novo para continuar.

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
row-unsaved-rule = não salvo · E para retomar
row-try-again = Tentar de novo
menu-mode = Claro ou escuro
menu-preview = Ver o app como um membro sem poderes de admin
row-menu-edit = Editar
row-menu-edit-rule = Editar regra
row-menu-paid = Marcar como pago
row-menu-unpaid = Desmarcar pago
row-menu-received = Marcar como recebido
row-menu-unreceived = Desmarcar recebido
row-menu-give = Passar para…

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
group-order-needs-group = ledger.group = "none" — S ordena os grupos; use :group bucket antes
not-a-command = não é um comando: { $name } (tente help)
no-rule-behind-row = não foi gerado por uma regra recorrente
read-only-ledger = somente leitura: este é o livro de { $who }
reattribute-admin-only = só um admin pode passar um lançamento para outro membro
pane-read-only = somente leitura

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
password-strength-weak = Fraca
password-strength-medium = Média
password-strength-strong = Forte
password-match = As senhas são iguais.
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
nothing-to-undo = nada seu para desfazer neste livro, neste mês
nothing-to-redo = nada para refazer
undone = desfeito
undo-did = desfeito: { $verb } "{ $title }"
redo-did = refeito: { $verb } "{ $title }"
undo-redo-hint = U refaz
redo-undo-hint = u desfaz
undo-more = { $n ->
        [one] + 1 campo
       *[other] + { $n } campos
    }
undo-confirm = u de novo para desfazer: { $verb } "{ $title }", de { $date }
redo-confirm = U de novo para refazer: { $verb } "{ $title }", de { $date }
undo-skipped = { $who } mudou "{ $title }" depois — ficou como deixou
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
rule-pdf-password = Senha do PDF
rule-pdf-password-hint = para faturas que só abrem com senha — os arquivos são guardados já abertos
rule-pdf-password-show = Mostrar senha
rule-pdf-password-hide = Esconder senha
rule-private = Escondido dos outros membros
rule-amount-in-filename = Valor no nome do arquivo
rule-create = Criar
rule-save = Salvar
rule-delete = Excluir regra
rule-path-legend = categoria / conta / identificação / ano / mês.ext
rule-past-note = Os meses anteriores, e este depois de pago, ficam como foram registrados. A linha deste mês segue o novo vencimento enquanto não estiver paga.
rule-created = regra criada · { $n } mês(es) gerado(s)
rule-saved = regra salva
rule-moved-due = a linha deste mês agora vence em { $date }
rule-deleted = regra excluída · as linhas continuam
rule-none-on-row = não vem de uma regra recorrente
rule-deleted-on-row = a regra desta linha foi excluída · arquivos e situação continuam valendo
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
file-pdf-locked = este PDF só abre com senha — preencha a senha do PDF na regra e anexe de novo — nada foi salvo
file-pdf-wrong-password = a senha do PDF da regra não abre este arquivo — confira na regra — nada foi salvo
file-pdf-no-tool = este PDF só abre com senha, e o qpdf não está instalado no servidor para abri-lo — nada foi salvo
file-pdf-unreadable = não consegui abrir este PDF — talvez esteja corrompido — nada foi salvo
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
pane-maximize = Maximizar · zz
pane-restore = Restaurar · zz

profile-appearance = Aparência
profile-ledger = Livro
profile-layout = Layout
profile-privacy = Linhas novas
profile-theme = Tema de cores
profile-line-numbers = Números de linha
status-version = A versão que este servidor está rodando
profile-panes = Painéis
profile-min-split = Abaixo desta largura, usar abas
profile-min-width = Largura mínima do painel
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
profile-density = Densidade
density-default = Padrão
density-comfortable = Confortável
density-compact = Compacta
profile-fold-default = Grupos começam
profile-fold-expanded = Abertos
profile-fold-collapsed = Fechados
profile-group-order = Grupos ordenados
profile-group-order-appearance = Ordem de aparição
profile-group-order-name = A–Z
profile-group-order-subtotal = Por custo
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
household-new-members = Para novos membros
household-new-members-note = Copiado para as configurações de cada novo membro quando ele entra. As configurações de quem já está aqui não mudam.
profile-use-household = Usar os padrões da casa
profile-use-household-note = A paleta, o layout e a densidade que a casa dá aos novos membros.
profile-used-household = agora usando a paleta, o layout e a densidade da casa
household-storage-note = Edite o ffm.toml e reinicie para mudar isto.

members-role-admin = Administrador
members-role-member = Membro
members-invite-label = Para quem (opcional)
members-invite-create = Criar link de convite
members-invite-note = Você nunca digita a senha de outra pessoa. Ela define na hora de aceitar o link, junto com o nome.
members-copy = Copiar
members-pending = Links pendentes
members-expires = expira em { $date }
members-revoke = Revogar
members-reset = Redefinir senha
members-signout = Desconectar de tudo
members-reset-note = Redefinir a senha desconecta { $name } de tudo e fica registrado no histórico.
members-signed-out = { $name } foi desconectado de { $n } aparelho(s)
members-signout-others = Desconectar outros aparelhos
members-signed-out-others = desconectado de { $n } outro(s) aparelho(s) — este continua
members-no-sessions = não está conectado em lugar nenhum
members-tab-active = Ativos
members-tab-inactive = Inativos
members-tab-trash = Lixeira
members-disable = Desativar
members-enable = Reativar
members-to-trash = Mover para a lixeira
members-restore = Restaurar
members-disable-note = Desativar { $name } encerra todas as sessões e esconde o painel — e o dinheiro dele — deste mês em diante. Os meses anteriores continuam mostrando.
members-inactive-since = inativo desde { $month }
members-inactive-none = Ninguém está desativado.
members-trash-none = A lixeira está vazia.
members-trash-note = Só vai para a lixeira quem não tem nenhuma linha — ela é para um engano. O nome continua ocupado enquanto estiver aqui.
members-link-days = Os links param de funcionar { $n } dias depois de criados.
members-copied = copiado
members-disabled = { $name } foi desativado
members-enabled = { $name } foi reativado
members-trashed = { $name } está na lixeira
members-restored = { $name } voltou
members-not-yourself = Esse é você — não é algo para fazer com a própria conta.
members-last-admin = A casa ficaria sem nenhum admin.
members-unknown = Esse membro não está mais aqui — o painel estava desatualizado.
members-has-rows = Há linhas no livro, então não pode ir para a lixeira. Desative em vez disso.
login-error-inactive = Esta conta está desativada. Peça a um admin para reativá-la.

command-placeholder = comando · Tab completa · Ctrl-p recupera
command-about-help = os atalhos — o mesmo que ?
command-about-profile = suas configurações
command-about-sort = a ordem das linhas
command-about-group = agrupar as linhas por categoria, ou desagrupar
command-about-set = números de linha: relativos · absolutos · nenhum
command-about-theme = sua paleta de cores — sozinho, abre o seletor
command-about-lang = seu idioma — só as palavras, nunca datas ou dinheiro
command-about-grep = buscar em todos os meses — sozinho, repete a última busca
command-about-trash = linhas apagadas e arquivos removidos, para trazer de volta
command-about-prune = arquivos duplicados — remover as cópias na lixeira
command-about-household = as configurações da casa
command-about-members = membros e links de convite
command-about-invite = um novo link de convite
command-about-currency = a moeda da casa e o formato dos números
command-about-preview = ver o app como um membro sem direitos de admin, e voltar
command-about-logout = encerrar esta sessão
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
log-action-import = lido de uma conta
log-action-materialize = gerou
log-action-password-reset = redefiniu a senha
log-action-sessions-revoked = desconectou de tudo

log-field-amount = valor
log-field-paid = pago à mão
log-field-title = descrição
log-field-date = data
log-field-due-day = dia de vencimento
log-field-due-date = vencimento
log-field-bucket = categoria
log-field-bill-name = nome da conta
log-field-instance = identificação
log-field-issuer = emissor
log-field-card = cartão
log-field-pdf-password = senha do PDF
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

## A lixeira — Ctrl-u / :trash

trash-title = Lixeira
trash-empty = Nada foi excluído. Linhas excluídas com dd e arquivos removidos vêm parar aqui.
trash-count = { $n ->
        [one] 1 item
       *[other] { $n } itens
    }
trash-more = só os mais recentes aparecem
trash-deleted = excluído { $time }
trash-deleted-by = excluído { $time } · { $who }
trash-replaced = substituído { $time }
trash-with-files = { $n ->
        [one] + 1 arquivo
       *[other] + { $n } arquivos
    }
trash-restore = Restaurar
trash-hint = x ou clique para marcar · X o dia · gx tudo · Enter restaura o que está marcado
trash-note = A linha volta para onde estava, com os arquivos dela. Um arquivo antigo troca de lugar com o atual, que vai para a lixeira no lugar dele.
trash-menu = Linhas excluídas e arquivos removidos, para trazer de volta
trash-nothing-picked = nada marcado — marque um item com x ou um clique
trash-slot-twice = Duas versões do mesmo arquivo estão marcadas. Escolha uma — só uma pode ficar na linha.
trash-rows = { $n ->
        [one] 1 linha
       *[other] { $n } linhas
    }
trash-files = { $n ->
        [one] 1 arquivo
       *[other] { $n } arquivos
    }
trash-restored = restaurado: { $what }
trash-elsewhere = { $n ->
        [one] 1 em outro mês
       *[other] { $n } em outros meses
    }
trash-failed = { $n ->
        [one] 1 não pôde ser restaurado
       *[other] { $n } não puderam ser restaurados
    }
trash-bill-elsewhere = "{ $title }" já está com a mesma conta, e uma conta só pode estar em uma linha

## Importação de contas

import-dropzone = Solte a conta aqui, ou escolha um arquivo
import-client-code = Código do cliente
import-unlink = Desvincular
import-read = li { $what } da conta
import-filled = li { $amount } com vencimento { $date } do código de barras
import-read-amount = li { $amount } do código de barras
import-amount-differs = o código de barras diz { $barcode }, esta linha diz { $row } — mantida como estava
import-date-differs = o código de barras diz { $barcode }, esta linha diz { $row } — mantida como estava
import-replaced = substituiu { $was }
import-unresolved = o código de barras não trazia { $what }
import-no-occurrence = não há linha deste mês para anexar
import-unlinked = conta desvinculada desta regra — solte a correta
import-already-attached = esta conta já está em { $period } ({ $title } — { $amount }) — nada foi salvo
import-already-attached-hidden = esta conta já está em { $period }, numa linha que você não vê — nada foi salvo
import-bill-moved = movida de { $title } { $period }
rule-path-missing-bucket = ‹categoria›
import-ask-mismatch = esta conta é de { $found }, mas { $rule } está vinculada a { $known } — anexar mesmo assim?
import-ask-unconfirmed = nada nesta conta confirma que é de { $rule } — sem código de barras, e o emissor não foi reconhecido. Anexar mesmo assim?
import-ask-unreadable = { $why } — nada confirma que é a conta de { $rule }. Anexar mesmo assim?
import-ask-title = Anexar esta conta?
import-ask-title-mismatch = Linha errada?
import-ask-hint = Só o y anexa. Qualquer outra tecla deixa de fora, e nada é salvo.
import-ask-yes = Anexar mesmo assim
import-ask-declined = não anexada — nada foi salvo
import-attached-anyway = anexada a { $rule } mesmo assim, como você pediu
import-why-no-text = este arquivo não tem texto para ler — uma digitalização ou uma foto
import-why-no-barcode = não há código de barras nele, e nenhum perfil de emissor o reconhece
import-why-no-barcode-known = não há código de barras nele, e as contas de { $issuer } sempre trazem um
import-why-misread = os dígitos verificadores do código de barras não batem — impresso ou lido errado
import-why-unreadable = não foi possível ler este arquivo
import-filled-text = li { $amount } com vencimento { $date } do texto da conta
import-read-amount-text = li { $amount } do texto da conta
import-id-account = conta { $code }
import-id-company = empresa { $code }
import-id-bank = um boleto do banco { $code }
import-id-card = cartão final { $digits }
import-meta-card = Cartão
import-meta-card-value = final { $digits }
import-meta-text-only = conta sem código de barras — lida do texto
import-meta-title = Conta vinculada
import-meta-hint = a que esta regra está vinculada — uma conta de qualquer outra coisa é perguntada antes
import-meta-issuer = Emissor
import-meta-kind = Código de barras
import-meta-company = Código da empresa
import-meta-kind-consumo = consumo / tributo
import-meta-kind-cobranca = boleto bancário
import-meta-pending = anexada quando você criar a regra
import-no-occurrence-new-rule = a regra começa depois deste mês, então a conta não foi anexada — anexe pelo E quando a primeira linha existir

## Seletor de mês

months-short = Jan Fev Mar Abr Mai Jun Jul Ago Set Out Nov Dez
month-picker = Mês
unresolved-dot = { $n ->
        [one] 1 conta vencida sem comprovante
       *[other] { $n } contas vencidas sem comprovante
    }
unresolved-here = vencidas aqui
month-picker-select = escolha o mês
month-future = nada registrado ainda
year-so-far = { $year } até agora · { $range }
year-full = { $year } · ano inteiro
year-avg = média { $amount } / mês
year-unresolved = { $n ->
        [one] 1 mês precisa de comprovante
       *[other] { $n } meses precisam de comprovante
    }

## `e` — editar a linha

edit-day = Dia
edit-private = Escondido dos outros membros
edit-paid = Pago — marcado à mão
edit-received = Recebido — marcado à mão
edit-saved = salvo
row-reattributed = passou para { $name }
row-now-private = escondido dos outros
paid-marked = marcado como pago à mão · u desfaz
paid-unmarked = não está mais marcado como pago
received-marked = marcado como recebido à mão · u desfaz
received-unmarked = não está mais marcado como recebido
paid-refused-tracked = uma conta acompanhada é paga pelo comprovante no disco, não à mão — gu3 anexa
paid-refused-one-off = só uma conta recorrente é marcada como paga à mão
row-now-shared = compartilhado com a casa
# `ga` — the member picker that re-attribution opens.
give-title = Passar para qual membro?
give-private = Esta linha está escondida dos outros. Quem recebê-la vira o dono, e você deixa de vê-la.
give-nobody = não há outro membro para quem passar
give-moved-key = agora um lançamento passa para outro membro com ga
drafts-parked = { $n ->
        [one] 1 rascunho não salvo
       *[other] { $n } rascunhos não salvos
    }

month-leave-key = mudar de mês descarta { $drafts } — { $key } de novo para ir mesmo assim
month-leave-click = mudar de mês descarta { $drafts } — clique de novo para ir mesmo assim
month-already-current = já está no mês atual

## `gd` e o carrossel `P`

details-due = Vence
details-cadence = Frequência
details-receipt = Comprovante
details-status = Situação
details-rule = Regra
details-rule-deleted = excluída em { $date }
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
carousel-size-title = Tamanho do arquivo
carousel-view-pages = Páginas
carousel-view-pdf = PDF
carousel-view-title = Ver as páginas como imagens, ou abrir o PDF no visualizador do navegador
carousel-page-alt = página { $n }
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

## `x` - the selection cell in the statusline
selection-count = { $n } de { $total } { $n ->
    [one] selecionada
   *[other] selecionadas
}
selection-no-amount = { $n } sem valor

# --- ? legenda: o que cada marca quer dizer ----------------------------------
legend-title = Marcas
legend-paid = Pago — o comprovante está no arquivo. Numa entrada: recebido.
legend-manual-paid = Marcado como pago à mão — uma conta sem arquivo que prove
legend-pending = Aguardando — o anel enche do início do mês até o vencimento, muda de cor quando está perto e fica vermelho quando vence. Numa entrada: esperado.
legend-scheduled = Agendado — o comprovante de agendamento está no arquivo, ainda não venceu
legend-scheduled-overdue = Agendado, mas vencido — o vencimento passou e o comprovante não chegou
legend-plain = Sem acompanhamento — nenhum arquivo é esperado, não há o que provar
legend-estimated = Estimado — uma entrada que varia, projetada pelos últimos meses até chegar
legend-recurring = Recorrente — criado por uma regra todo mês. Apagado: avulso, ou a regra foi excluída.
legend-private = Privado — escondido dos outros membros, nunca do admin. Apagado: a casa toda vê.
legend-files = Arquivos da linha — 1 conta, 2 agendamento, 3 comprovante. g1 g2 g3 abrem, P mostra todos.

# --- e: arquivos da linha, dentro da edição -----------------------------------
edit-files = Arquivos
edit-file-open = Abrir em nova aba
edit-file-remove = Remover

# --- P: remover o arquivo em exibição ------------------------------------------
carousel-remove = Remover
carousel-remove-title = Remover este arquivo ({ $file })?
carousel-remove-body = Ele vai para a lixeira do seu cofre, nunca sai do disco. Aperte u para trazer de volta.
carousel-remove-yes = Remover

# --- the loader: a request in flight -------------------------------------------
busy-label = Carregando…
busy-offline = O servidor não respondeu — nada foi alterado. Tente de novo.

## Que tipo de servidor é este — o selo de um build de desenvolvimento (`crate::run`)

run-dev-title = Um build de desenvolvimento — não é o servidor da casa. O que você muda aqui não é o livro de verdade.
run-dev-lan-title = Um build de desenvolvimento, aberto à rede local — não é o servidor da casa.
run-demo-title = Um build de desenvolvimento rodando como se fosse { $date }, numa cópia do banco — nada aqui chega ao livro de verdade.

## Live updates — a row changed while this page was open

live-from-disk = atualizado a partir do disco — um arquivo foi colocado ou removido à mão
live-under-edit = os arquivos desta linha mudaram no disco — Esc para vê-los

## Duplicate files — :prune

prune-title = Duplicados
prune-count = { $n ->
        [one] 1 grupo
       *[other] { $n } grupos
    } · { $size } podem sair
prune-group = { $hash } · { $n } cópias · { $size }
prune-empty = Nenhum arquivo do cofre se repete. Nada para limpar.
prune-hint = Só cópias numa lixeira podem ser marcadas — o que está em uso fica · x ou clique para marcar · X o grupo · gx tudo · Enter remove
prune-note = Cada cópia aqui tem os mesmos bytes de uma que fica, então removê-la não perde nada. Ela sai do disco de vez: não vai para a lixeira, e u não a traz de volta.
prune-live = em uso · { $row }
prune-live-unknown = em uso · no disco, sem registro
prune-trash = lixeira · { $row } · excluído em { $day }
prune-trash-unknown = lixeira · sem registro
prune-remove = Remover
prune-confirm = Remover de vez as cópias marcadas?
prune-confirm-body = Todas têm os mesmos bytes de um arquivo que fica. Elas não vão para a lixeira — saem do disco, e u não as traz de volta.
prune-confirm-yes = Remover de vez
prune-nothing-picked = nada marcado — x ou clique numa cópia antes
prune-done = { $n ->
        [one] 1 cópia removida
       *[other] { $n } cópias removidas
    } · { $size } liberados
prune-not-in-trash = { $path } não está numa lixeira — só uma cópia ali pode ser removida
prune-not-a-duplicate = { $path } não é mais cópia de nada — a lista estava desatualizada, veja de novo
prune-last-copy = todas as cópias de { $path } estão marcadas — deixe uma
prune-failed = não foi possível remover todas as cópias — a lista mostra o que restou

## The totals' words — lowercase; the app bar and the picker uppercase them

totals-in = entradas
totals-out = saídas
totals-net = saldo
totals-balance = saldo

# --- a vista de recortar e reduzir ------------------------------------------------
prepare-title = Antes de arquivar — fique com as páginas que importam
prepare-title-stored = Recortar ou reduzir este arquivo
prepare-remove = Remover
prepare-keep = Manter
prepare-shrink = Reduzir
prepare-hint = x manter / tirar · h l páginas
prepare-spans = Uma página por mês — todas ficam
prepare-save = Salvar
prepare-keep-one = Fique com pelo menos uma página
prepare-failed = Não foi possível calcular o tamanho — o arquivo ficou como estava
prepare-shrink-no-gain = não diminui — mantido
prepare-not-pdf = Só um PDF pode ser recortado ou reduzido
prepare-trim = Recortar / reduzir
