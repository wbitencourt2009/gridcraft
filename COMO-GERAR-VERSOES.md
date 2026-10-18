# Como gerar as versões do GridCraft (Windows, Mac e Linux)

## Windows: instalador sem administrador (feito nesta máquina)

O instalador fica em `dist\gridcraft-<versão>-windows-x64-user-setup.exe`.

- Instala em `%LOCALAPPDATA%\Programs\GridCraft`, **sem pedir senha de administrador**.
- Cria o atalho no Menu Iniciar (e na área de trabalho, se marcar a opção).
- Aparece em "Abrir com" para `.xlsx`, `.xlsm`, `.xlsb`, `.ods`, `.csv` e `.tsv`, sem virar o app padrão.
- Desinstala em **Configurações → Aplicativos**.
- Não é assinado digitalmente: na primeira vez o Windows SmartScreen pode avisar
  "O Windows protegeu o computador". Clique em **Mais informações → Executar assim mesmo**.

Para gerar de novo (com o Inno Setup instalado em `%LOCALAPPDATA%\Programs\Inno Setup 6`):

```powershell
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = "-C target-feature=+crt-static"
cargo build --release --locked -p gridcraft -p gridcraft-cli --target x86_64-pc-windows-msvc
& "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe" /DVersion=0.7.0 /DBinDir="$PWD\target\x86_64-pc-windows-msvc\release" /DOutDir="$PWD\dist" packaging\windows\gridcraft-user.iss
```

## Mac e Linux: pelo GitHub Actions (grátis)

O código para Mac só pode ser compilado num Mac, e o GitHub empresta máquinas Mac e Linux de graça.
O workflow `.github/workflows/build-unsigned.yml` gera as três versões de uma vez.

1. **Crie seu repositório.** Entre em github.com/storytold/gridcraft e clique em **Fork**
   (ou crie um repositório vazio seu, por exemplo `gridcraft`).
2. **Envie este código.** Nesta pasta (`C:\Users\Wbitencourt\gridcraft`), com as alterações já
   salvas num commit:

   ```bash
   git remote add meu https://github.com/SEU-USUARIO/gridcraft.git
   git push meu ptbr-build
   ```

   O Git abre o navegador para você entrar na sua conta do GitHub.
3. **Ligue o Actions.** No seu repositório, abra a aba **Actions**. Num fork, clique em
   "I understand my workflows, go ahead and enable them".
4. **Rode o build.** Escolha **Build (unsigned, all platforms)** → **Run workflow** → branch
   `ptbr-build` → **Run workflow**. Leva uns 20–30 minutos.
5. **Baixe os arquivos.** Abra a execução concluída e baixe em **Artifacts**:
   - `gridcraft-windows-x64`: instalador sem administrador + versão portátil (.zip)
   - `gridcraft-macos-universal`: `.dmg` para Mac com chip Apple ou Intel
   - `gridcraft-linux-x86_64`: `.AppImage` (qualquer distro), `.deb` (Ubuntu/Debian/Mint),
     `.rpm` (Fedora/openSUSE) e `.tar.gz`

## Avisos na primeira abertura (versões sem assinatura)

- **Mac:** a versão não é autenticada pela Apple. Abra o `.dmg`, arraste o GridCraft para
  Aplicativos e, na primeira vez, clique com o **botão direito → Abrir → Abrir**. Se o macOS
  bloquear, vá em **Ajustes do Sistema → Privacidade e Segurança → Abrir Mesmo Assim**.
- **Linux (AppImage):** `chmod +x gridcraft-*.AppImage` e execute. Algumas distros precisam do
  pacote `libfuse2`. Pelo `.deb`: `sudo apt install ./gridcraft-*.deb`.
- **Windows:** SmartScreen → **Mais informações → Executar assim mesmo**.

As versões oficiais assinadas exigem os certificados da organização dona do GridCraft
(veja `docs/releasing.md`).
