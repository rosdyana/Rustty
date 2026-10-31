# Política de firma de código

Esta página describe cómo se firman los binarios de Rustty, quién autoriza cada
firma y cómo comprobar que una descarga es auténtica.

> La firma de código de los binarios de **Windows** está **en evaluación**.
> Mientras tanto, el ejecutable portable de Windows se publica sin firma
> Authenticode; verifícalo con el `SHA256SUMS.txt` del release, que sí está
> respaldado por una attestation de procedencia.

## Estado de la firma por plataforma

- **Windows** (`.exe` portable): firma de código **en evaluación**. Hasta
  entonces, verifícalo con `SHA256SUMS.txt`.
- **Debian / Ubuntu** (`.deb`): no aplica firma de código del sistema operativo;
  la integridad se valida con `SHA256SUMS.txt`.
- **`SHA256SUMS.txt`**: lleva una **attestation de procedencia** de GitHub que lo
  ata al workflow, al commit y al runner que generaron el release.

Rustty no se actualiza solo: comprueba si hay una versión nueva y, si la hay,
abre la página del release para que la descargues tú.

## Construcción verificable

Todos los binarios oficiales se construyen **a partir del código fuente público**
mediante GitHub Actions al publicar una etiqueta de versión. No se firman binarios
construidos en equipos locales y cada artefacto lleva sus metadatos (nombre de
producto y versión) fijados desde la fuente única de versión del repositorio.

## Roles y aprobación

Rustty está mantenido por **Alejandro Soriano**
([@Aleixenandros](https://github.com/Aleixenandros)), que asume los roles de
**autor/committer**, **revisor** y **aprobador**. **Cada release requiere
aprobación manual antes de firmarse**: ningún artefacto se firma de forma
automática sin validar esa versión concreta. Las cuentas con acceso al
repositorio y a la firma usan **autenticación multifactor**.

## Cómo verificar una descarga

- **`SHA256SUMS.txt`**: comprueba primero que el manifiesto viene del CI del
  proyecto y después que tu descarga coincide con él.

  ```bash
  gh attestation verify SHA256SUMS.txt --repo rosdyana/Rustty
  sha256sum --check --ignore-missing SHA256SUMS.txt
  ```

- **Windows** (cuando la firma esté activa): el editor verificado aparecerá en
  SmartScreen / UAC y en *Propiedades → Firmas digitales*.

## Privacidad

Rustty es *local-first* y **no transmite información a otros sistemas en red salvo
que lo solicite expresamente la persona que lo instala o lo opera**. Consulta la
[política de privacidad](/politica-privacidad) para el detalle.

## Reportar un problema de firma

Si un binario que dice ser de Rustty tiene una firma o un `sha256` que no coincide
con lo publicado, **no lo ejecutes** y repórtalo de forma privada según el
[`SECURITY.md`](https://github.com/rosdyana/Rustty/blob/main/SECURITY.md) del
repositorio.

El detalle técnico completo está en
[`CODE_SIGNING.md`](https://github.com/rosdyana/Rustty/blob/main/CODE_SIGNING.md).
