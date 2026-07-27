@echo off
title Apache vs App Diagnostic
color 0A
setlocal enabledelayedexpansion

:: ============================================================
::  CONFIGURACION (ajusta si es necesario)
:: ============================================================
set APACHE_DIR=C:\devpanel\bin\apache\httpd-2.4.68-260617-Win64-VS18
set APACHE_BIN=%APACHE_DIR%\bin\httpd.exe
set DOCUMENT_ROOT=C:\devpanel\www\demotest
set PHP7_DIR=C:\devpanel\bin\php\php-7.4.33-Win32-vc15-x64
set PHP8_3_DIR=C:\devpanel\bin\php\php-8.3.32-Win32-vs16-x64
set PHP8_4_DIR=C:\devpanel\bin\php\php-8.4.23-Win32-vs17-x64
set CURL=%APACHE_DIR%\bin\curl.exe
set TEST_HTML=test_static.html
set TEST_PHP=test_apache.php
set TEST_URL_HTTP=http://demotest.dev
:: ============================================================

cd /d "%APACHE_DIR%"

echo ===================================================
echo   DIAGNOSTICO COMPLETO - APACHE vs APP
echo   Fecha: %date% %time%
echo ===================================================
echo.

:: ---------- 1. Verificar sintaxis ----------
echo [1] Verificando sintaxis de Apache...
"%APACHE_BIN%" -d "%APACHE_DIR%" -C "Listen 80" -t
if errorlevel 1 (
    echo   ERROR DE SINTAXIS. Corrige antes de continuar.
    goto :fin
) else (
    echo   OK Sintaxis correcta.
)
echo.

:: ---------- 2. Mostrar VirtualHosts ----------
echo [2] VirtualHosts activos:
"%APACHE_BIN%" -d "%APACHE_DIR%" -C "Listen 80" -S
echo.

:: ---------- 3. Listar modulos clave ----------
echo [3] Modulos importantes cargados:
"%APACHE_BIN%" -d "%APACHE_DIR%" -M | findstr /i "rewrite ssl proxy php"
echo.

:: ---------- 4. Detectar PHP en Apache ----------
echo [4] Detectando integracion con PHP...
set PHP_LOADED=0
set PHP_METHOD=ninguno
"%APACHE_BIN%" -d "%APACHE_DIR%" -M | findstr /i "php" >nul
if errorlevel 1 (
    echo   No se encontro modulo PHP cargado (mod_php).
) else (
    set PHP_LOADED=1
    set PHP_METHOD=mod_php
    echo   Modulo PHP cargado.
)
:: Verificar si hay proxy_fcgi
"%APACHE_BIN%" -d "%APACHE_DIR%" -M | findstr /i "proxy_fcgi" >nul
if not errorlevel 1 (
    echo   proxy_fcgi_module presente (posible PHP-FPM o FastCGI).
    set PHP_METHOD=proxy_fcgi
)
:: Buscar en vhost si hay SetHandler o ProxyPassMatch para PHP
if exist "%APACHE_DIR%\conf\httpd.conf" (
    findstr /i "SetHandler.*php" "%APACHE_DIR%\conf\httpd.conf" >nul
    if not errorlevel 1 set PHP_METHOD=SetHandler
)
if exist "C:\devpanel\data\vhosts\apache\demotest.conf" (
    findstr /i "SetHandler.*php" "C:\devpanel\data\vhosts\apache\demotest.conf" >nul
    if not errorlevel 1 set PHP_METHOD=SetHandler
    findstr /i "ProxyPassMatch.*php" "C:\devpanel\data\vhosts\apache\demotest.conf" >nul
    if not errorlevel 1 set PHP_METHOD=ProxyPassMatch
)
echo   Metodo detectado: %PHP_METHOD%
echo.

:: ---------- 5. Probar PHP CLI ----------
echo [5] Probando PHP desde CLI:
for %%v in ("%PHP7_DIR%\php.exe" "%PHP8_3_DIR%\php.exe" "%PHP8_4_DIR%\php.exe") do (
    if exist "%%~v" (
        echo   %%~nv:
        "%%~v" -v 2>nul | findstr /i "PHP" | head -n 1
        "%%~v" -r "echo 'CLI OK';" 2>nul
    ) else (
        echo   No encontrado: %%~v
    )
)
echo.

:: ---------- 6. Estado del servicio Apache ----------
echo [6] Estado del servicio Apache:
sc query Apache2.4 2>nul | findstr /i "STATE" >nul
if errorlevel 1 (
    echo   No se encontro el servicio 'Apache2.4'. Verificando proceso...
    tasklist /FI "IMAGENAME eq httpd.exe" 2>nul | findstr httpd.exe >nul
    if errorlevel 1 (
        echo   Apache NO esta corriendo (ni servicio ni proceso).
    ) else (
        echo   Apache esta corriendo como proceso (httpd.exe).
    )
) else (
    for /f "tokens=3" %%a in ('sc query Apache2.4 ^| findstr /i "STATE"') do set STATE=%%a
    echo   Estado del servicio: %STATE%
    if /i "%STATE%"=="RUNNING" (echo   Servicio en ejecucion.) else (echo   Servicio NO esta en ejecucion.)
)
echo.

:: ---------- 7. Crear y probar archivo HTML estatico ----------
echo [7] Probando contenido estatico...
if not exist "%DOCUMENT_ROOT%" (
    echo   ERROR: DocumentRoot no existe: %DOCUMENT_ROOT%
    goto :skip_html
)
echo <html><body><h1>Prueba estatica - OK</h1></body></html> > "%DOCUMENT_ROOT%\%TEST_HTML%"
echo   Creado %TEST_HTML%
echo   Accediendo a %TEST_URL_HTTP%/%TEST_HTML% ...
%CURL% -s -o nul -w "HTTP Code: %%{http_code}" "%TEST_URL_HTTP%/%TEST_HTML%"
echo.
echo   (Si codigo es 200, Apache sirve HTML estatico correctamente.)
:skip_html
echo.

:: ---------- 8. Crear y probar archivo PHP ----------
echo [8] Probando procesamiento PHP (phpinfo)...
(
echo ^<?php
echo phpinfo();
echo ?^>
) > "%DOCUMENT_ROOT%\%TEST_PHP%"
echo   Creado %TEST_PHP%
echo   Accediendo a %TEST_URL_HTTP%/%TEST_PHP% ...
%CURL% -s -o "%TEMP%\phpinfo_output.html" -w "HTTP Code: %%{http_code}" "%TEST_URL_HTTP%/%TEST_PHP%"
echo.
if exist "%TEMP%\phpinfo_output.html" (
    findstr /i "PHP Version" "%TEMP%\phpinfo_output.html" >nul
    if errorlevel 1 (
        echo   NO se encontro 'PHP Version' en la respuesta.
        echo   Posiblemente PHP no se esta procesando.
        echo   Ultimas lineas de la respuesta:
        type "%TEMP%\phpinfo_output.html" | tail -n 5
    ) else (
        echo   PHP SI se esta procesando. Version:
        findstr /i "PHP Version" "%TEMP%\phpinfo_output.html"
    )
    del "%TEMP%\phpinfo_output.html" 2>nul
) else (
    echo   No se recibio respuesta. Verifica que Apache este corriendo y el vhost sea accesible.
)
echo.

:: ---------- 9. Revisar error.log ----------
echo [9] Ultimas lineas del error.log (con errores PHP o Apache):
set ERROR_LOG=%APACHE_DIR%\logs\error.log
if exist "%ERROR_LOG%" (
    echo   (mostrando hasta 10 lineas que contengan 'error' o 'php')
    type "%ERROR_LOG%" | findstr /i "error php" | tail -n 10
) else (
    echo   No se encontro error.log en %ERROR_LOG%
)
echo.

:: ---------- 10. Resumen y conclusiones ----------
echo [10] RESUMEN:
echo   - Sintaxis Apache: OK (si no hubo error)
echo   - Metodo PHP detectado: %PHP_METHOD%
echo   - Prueba estatica: revisa codigo HTTP arriba (200 = OK)
echo   - Prueba PHP: revisa si se muestra 'PHP Version' arriba.
echo.
echo   Si la estatica funciona y la PHP falla:
echo     - El problema es la integracion Apache-PHP (configuracion, modulo, FPM).
echo   Si ambas funcionan pero tu app falla:
echo     - El problema es tu codigo, base de datos, o dependencias.
echo   Si ambas fallan:
echo     - Apache no esta sirviendo el vhost correctamente (DNS, puerto, permisos).
echo.
echo   Si usas PHP-FPM (proxy_fcgi), asegurate de que el servicio FPM este corriendo.
echo   Puedes probar con: %PHP8_4_DIR%\php-cgi.exe -b 127.0.0.1:9000 (u otro puerto)
echo.

:: ---------- 11. Limpieza opcional ----------
echo [11] Eliminar archivos de prueba? (S/N)
set /p resp=
if /i "%resp%"=="S" (
    del "%DOCUMENT_ROOT%\%TEST_HTML%" 2>nul
    del "%DOCUMENT_ROOT%\%TEST_PHP%" 2>nul
    echo   Archivos eliminados.
) else (
    echo   No eliminados. Puedes borrarlos manualmente.
)

:fin
echo.
echo ===================================================
echo   Diagnostico completado.
echo ===================================================
pause