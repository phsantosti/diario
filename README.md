# Diário

Diário de trabalho por voz, para Windows. Clique em **Falar**, conte o que fez, clique de novo e o texto aparece na caixa. Corrija se precisar e salve. Na hora da daily, abra o dia anterior e clique em **Copiar para a daily**.

A transcrição roda no seu computador com o [Whisper](https://github.com/ggml-org/whisper.cpp), offline. Nada do que você fala sai da máquina.

## Uso

- **🎤 Falar / ⏹ Parar e transcrever**: grava e transcreve. Pode gravar várias vezes seguidas; o texto se acumula na caixa.
- **💾 Salvar no diário** (Ctrl+Enter): vira uma anotação com a hora atual no dia aberto. Para registrar algo esquecido de ontem, abra ontem e salve lá.
- **✏ / 🗑**: editar ou apagar uma anotação (apagar pede confirmação).
- **◀ ▶ Hoje** e a lista **Dias anotados**: navegar entre os dias. Na segunda-feira, a sexta aparece no topo da lista.
- **📋 Copiar para a daily**: copia as anotações do dia aberto como lista, por exemplo:

  ```text
  Ontem (24/11):
  - Revisei o pull request da tela de pedidos.
  - Reunião com o suporte sobre o chamado do relatório.
  ```

- O microfone fica no canto de cima. A escolha é lembrada.
- O texto da caixa é guardado em disco enquanto você digita, então fechar o app não perde uma transcrição.

Na primeira execução o app baixa o modelo de voz (466 MB). Depois disso funciona sem internet.

## Onde ficam os dados

Em `%LOCALAPPDATA%\Diario\` (troque com a variável `DIARIO_DIR`):

- `entradas\AAAA-MM-DD.md`: um arquivo Markdown por dia, com uma seção `## HH:MM` por anotação. Dá para abrir e editar à mão.
- `modelos\`: o modelo de voz.

## Modelo de voz

O padrão é `ggml-small.bin`, que transcreve 17 s de fala em cerca de 5 s num processador de 16 núcleos. Para trocar, defina `DIARIO_MODELO` com outro arquivo de https://huggingface.co/ggerganov/whisper.cpp (por exemplo `ggml-large-v3-turbo-q5_0.bin`, mais preciso e mais lento). O app baixa o novo modelo na próxima vez que abrir.

Para testar um modelo com um arquivo WAV e medir o tempo:

```powershell
cargo run --release --example transcrever -- caminho\arquivo.wav [threads] [feixe]
```

## Compilar

Precisa de:

- [Rust](https://rustup.rs) com o toolchain MSVC;
- Visual Studio (ou Build Tools) com C++;
- CMake, para compilar o whisper.cpp. O Visual Studio já traz um em `Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin`; se ele não estiver no PATH, aponte a variável `CMAKE` para o `cmake.exe`;
- libclang, para gerar os bindings do whisper.cpp. Instale o LLVM (`winget install LLVM.LLVM`) ou pegue só a DLL no pacote [`libclang`](https://pypi.org/project/libclang/) do PyPI (o `.whl` `win_amd64` é um zip; a DLL fica em `clang/native/libclang.dll`) e aponte `LIBCLANG_PATH` para a pasta dela.

```powershell
cargo build --release
cargo test --release
.\target\release\diario.exe
```

A `.cargo\config.toml` passa `/O2` ao compilar o whisper.cpp. Sem isso o cmake-rs compila sem otimização e a transcrição fica cerca de 5x mais lenta.
