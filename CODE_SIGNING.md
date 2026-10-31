# Política de firma de código

Este documento describe cómo se firman los binarios de Rustty, quién puede
autorizar una firma y cómo verificar que un artefacto descargado es auténtico.

> Estado: el ejecutable portable de **Windows** se publica por ahora **sin firma
> Authenticode**; la firma de código de Windows está **en evaluación** (ver «Plan
> de firma de Windows»). La verificación por `SHA256SUMS.txt` con attestation de
> procedencia ya está en vigor para todos los artefactos.

## Estado actual de la firma

| Plataforma | Firma | Mecanismo |
| --- | --- | --- |
| **Windows** (`Rustty_<version>_x64-portable.exe`) | **Todavía no** (firma en evaluación) | Pendiente de certificado. Verifica el ejecutable contra `SHA256SUMS.txt`. |
| **Debian / Ubuntu** (`Rustty_<version>_amd64.deb`) | No aplica firma de código de SO | La integridad se valida con `SHA256SUMS.txt` y su attestation de procedencia. |
| **Manifiesto `SHA256SUMS.txt`** (todas las plataformas) | Sí | **Attestation de procedencia** de GitHub (Sigstore), emitida por el workflow de release; cubre por hash todos los artefactos. |

Rustty no tiene actualizador automático: la app solo consulta la API de releases
de GitHub y, si hay una versión nueva, abre la página del release para que la
descargues. Por eso no hay firmas de actualizador ni secretos asociados.

Los secretos que necesite el futuro firmado de Windows vivirán exclusivamente en
los secretos cifrados de GitHub Actions del repositorio y nunca se incluirán en el
código fuente ni en los artefactos publicados.

## Plan de firma de Windows

La firma de código de Windows está **en evaluación**. Mientras tanto, los
ejecutable portable (`.exe`) se publica sin firma Authenticode y debe
verificarse contra `SHA256SUMS.txt`. Las vías que se consideran:

- **SignPath Foundation** (gratuita para OSS) — objetivo preferido. Rustty cumple
  los criterios técnicos de elegibilidad (licencia [Apache-2.0](LICENSE) aprobada
  por la OSI sin doble licencia, sin componentes propietarios más allá de las
  librerías del sistema, mantenido activamente, publicado y documentado). El
  programa, además, valora la **visibilidad y reputación pública** del proyecto
  (adopción, referencias externas), por lo que la concesión queda supeditada a que
  Rustty alcance ese umbral; se reaplicará cuando así sea.
- **Certificado de validación individual** de una CA comercial (p. ej.
  [Certum Open Source](https://www.certum.eu/en/code-signing-certificates/) o
  [SSL.com](https://www.ssl.com/) IV con eSigner), que verifica la identidad del
  autor y no exige reputación del proyecto. Implica un coste anual.

Cuando exista un certificado, los binarios de Windows se firmarán dentro del flujo
de GitHub Actions descrito abajo y se actualizará la tabla de estado.

## Construcción verificable

Todos los binarios oficiales se construyen **a partir del código fuente público**
de este repositorio mediante GitHub Actions (`.github/workflows/build.yml`), que
se dispara al publicar una etiqueta `v*`. No se firman binarios construidos en
equipos locales. Cada artefacto lleva sus metadatos (nombre de producto y
versión) fijados desde la fuente única de versión (`package.json`, propagada por
`scripts/sync-version.mjs`). Los scripts de construcción y la configuración de CI
son públicos y están sujetos a revisión de código.

## Roles y aprobación de releases

Rustty está mantenido por **Alejandro Soriano** ([@Aleixenandros](https://github.com/Aleixenandros)),
que asume los tres roles definidos por la SignPath Foundation:

- **Autores / Committers**: personas con permiso para modificar el código fuente.
  Actualmente: [@Aleixenandros](https://github.com/Aleixenandros).
- **Revisores (Reviewers)**: revisan los cambios procedentes de personas que no
  son committers antes de integrarlos. Actualmente: [@Aleixenandros](https://github.com/Aleixenandros).
- **Aprobadores (Approvers)**: deciden si una release puede firmarse.
  Actualmente: [@Aleixenandros](https://github.com/Aleixenandros).

**Cada release requiere aprobación manual antes de firmarse.** Ningún artefacto se
firma de forma automática sin que un aprobador valide explícitamente esa versión
concreta.

Todas las cuentas con acceso al repositorio de código fuente y a la firma de
código (GitHub y SignPath) tienen activada la **autenticación multifactor (MFA)**.

## Declaración de privacidad

Rustty es una aplicación *local-first* y **no transmite información a otros
sistemas en red salvo que lo solicite expresamente la persona que lo instala o lo
opera** (sus propias conexiones SSH/SFTP/RDP, el proveedor de sincronización que
configure o la comprobación de actualizaciones, todos opcionales y bajo su
control). Los detalles completos están en la
[política de privacidad](https://rustty.es/politica-privacidad).

## Cómo verificar la integridad de una descarga

- **Manifiesto `SHA256SUMS.txt` con procedencia**: cada release publica un
  `SHA256SUMS.txt` con el hash SHA-256 de todos sus artefactos, y ese manifiesto
  lleva una **attestation de procedencia** de GitHub (emitida por el job
  `publish_release`, fuera del propio release) que lo ata al workflow, al commit y
  al runner que lo generaron. Así, quien pudiera alterar un binario del release no
  puede alterar también el hash con el que se compara:

  ```bash
  # 1. El manifiesto lo generó el CI de este repositorio
  gh attestation verify SHA256SUMS.txt --repo rosdyana/Rustty

  # 2. Tu descarga (p. ej. Rustty_x.y.z_amd64.deb o
  #    Rustty_x.y.z_x64-portable.exe) coincide con el manifiesto
  sha256sum --check --ignore-missing SHA256SUMS.txt
  ```

  En Windows (PowerShell), el paso 2 equivale a comparar
  `Get-FileHash .\Rustty_*_x64-portable.exe` con su línea del manifiesto.
- **Windows** (cuando la firma esté activa): el binario mostrará el editor
  verificado en el diálogo de SmartScreen / UAC y en
  *Propiedades → Firmas digitales*.

## Atribución

Una vez activa la firma con la SignPath Foundation, el sitio del proyecto mostrará
la siguiente atribución, tal como exige el programa:

> Free code signing provided by [SignPath.io](https://about.signpath.io),
> certificate by [SignPath Foundation](https://signpath.org).

## Reporte de problemas de firma

Si encuentras un binario que dice ser de Rustty pero cuya firma o `sha256` no
coincide con lo publicado, **no lo ejecutes** y repórtalo de forma privada según
el procedimiento de [`SECURITY.md`](SECURITY.md).
