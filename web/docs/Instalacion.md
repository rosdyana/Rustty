# Instalación

Rustty publica dos binarios precompilados en la [página de releases](https://github.com/rosdyana/Rustty/releases):

| Sistema | Fichero | Formato |
|---|---|---|
| **Windows 10/11 (x64)** | `Rustty_<version>_x64-portable.exe` | Ejecutable portable, sin instalador |
| **Debian / Ubuntu (x86_64)** | `Rustty_<version>_amd64.deb` | Paquete `.deb` |

Son las dos únicas plataformas soportadas. En otros sistemas puedes [compilar desde fuente](#compilar-desde-fuente), pero sin soporte oficial.

Desde Rustty puedes comprobar si hay una versión nueva en **Preferencias → Acerca de**, o activar **Comprobar al iniciar** para que la app lo revise al arrancar. Rustty no se actualiza solo: si hay versión nueva, te avisa y abre la página del release para que descargues el fichero.

## Windows (portable)

En Windows 10 22H2 y Windows 11 el runtime **Microsoft Edge WebView2** viene preinstalado. Si tu sistema no lo tiene, instálalo desde la [página de Microsoft](https://developer.microsoft.com/microsoft-edge/webview2/) antes de abrir Rustty.

Descarga `Rustty_<version>_x64-portable.exe` y ejecútalo: es un único fichero, no necesita instalación ni permisos de administrador, y sirve también desde un USB o en equipos bloqueados. Puedes dejarlo en cualquier carpeta y crear un acceso directo.

Mientras el nombre del fichero termine en `-portable.exe`, Rustty guarda toda su configuración (perfiles, preferencias, notas…) en una carpeta `.conf\com.rustty.app\` **junto al propio ejecutable**, sin tocar `%APPDATA%`: copia la carpeta a otro equipo o a un USB y la configuración viaja con ella. Las contraseñas siguen en el Administrador de credenciales de Windows del usuario; para llevarlas contigo usa una base **KeePass** junto al `.exe` o la sincronización cifrada de contraseñas. Si renombras el `.exe` quitando el sufijo `-portable.exe`, vuelve al modo normal y usa `%APPDATA%\com.rustty.app\`.

> Los binarios de Windows aún **no están firmados** (la firma de código está en evaluación; ver la [política de firma de código](?page=Firma)). Mientras tanto, SmartScreen o algún antivirus pueden mostrar un aviso o un falso positivo: [verifica la descarga](#verificar-integridad) para confirmar que el fichero no ha sido alterado.

### Actualizar en Windows

Descarga el `.exe` portable de la versión nueva y ponlo **en la misma carpeta** que el anterior (puedes borrar el viejo): como su nombre también termina en `-portable.exe`, leerá la misma carpeta `.conf\` y conservarás tus datos.

## Debian / Ubuntu (.deb)

Rustty requiere **WebKitGTK 4.1** y **libayatana-appindicator** en tiempo de ejecución; `apt` los instala como dependencia del paquete. Funciona en Ubuntu 22.04 o posterior, Debian 12 o posterior y derivadas (Linux Mint, Pop!_OS…).

```bash
sudo apt install ./Rustty_*_amd64.deb
```

### Actualizar en Debian / Ubuntu

Descarga el `.deb` de la versión nueva e instálalo con el mismo comando: `apt` actualiza encima de la versión anterior y conserva tus datos.

## Verificar integridad

Cada release incluye un `SHA256SUMS.txt` con el hash de todos los artefactos, y ese manifiesto lleva una **attestation de procedencia** de GitHub que lo ata al workflow, al commit y al runner que lo generaron:

```bash
# 1. El manifiesto viene del CI del proyecto
gh attestation verify SHA256SUMS.txt --repo rosdyana/Rustty

# 2. Tu descarga coincide con el manifiesto
sha256sum --check --ignore-missing SHA256SUMS.txt
```

En Windows (PowerShell), el paso 2 equivale a comparar `Get-FileHash .\Rustty_*_x64-portable.exe` con la línea correspondiente de `SHA256SUMS.txt`.

Para el detalle de cómo se construyen los binarios y cómo verificarlos, consulta la [política de firma de código](?page=Firma).

## Clientes externos opcionales

SSH, SFTP, FTP y FTPS funcionan dentro de Rustty. Para **RDP**, **VNC** y **Telnet**, Rustty lanza clientes externos del sistema; instala solo los que vayas a usar.

En Debian / Ubuntu:

```bash
sudo apt install freerdp2-x11     # RDP
sudo apt install tigervnc-viewer  # VNC
sudo apt install telnet           # Telnet
```

Telnet en Linux necesita además un emulador de terminal (`x-terminal-emulator`, GNOME Terminal, Konsole, Xfce Terminal, Alacritty, Kitty o xterm).

En Windows, RDP usa `mstsc.exe` incluido en el sistema. VNC requiere un visor instalado o un manejador `vnc://`. Telnet puede requerir activar el **Cliente Telnet** desde las características opcionales.

Más detalle en [Clientes externos](?page=ClientesExternos).

## CLI SSH

Una vez instalado, el binario también permite listar perfiles SSH, abrir sesiones interactivas y ejecutar comandos remotos desde terminal. Consulta la [guía del CLI SSH](?page=CLI) para ejemplos y limitaciones.

## Compilar desde fuente

Si prefieres compilar tú mismo, consulta el [README del proyecto](https://github.com/rosdyana/Rustty#desarrollo-y-construcción) para las dependencias y los comandos de compilación.
