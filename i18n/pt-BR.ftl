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
