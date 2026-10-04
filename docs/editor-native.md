# Editor nativo de G0

`cargo run --offline --bin g0-editor -- documento.g0g` abre el editor Win32/GDI.
También admite programas `.g0p`. Abrir, editar y guardar reconstruye definiciones;
no concede autoridad ni ejecuta efectos. Ejecutar utiliza ResourceHost y las
mismas verificaciones de contratos que el ejecutor normal.

## Edición

La toolbox crea constantes Integer, Bool, Text y Bytes, y 33 operaciones puras:
aritmética, comparaciones, lógica, conversión checked, arrays, índices, longitud,
texto/bytes, UTF-8, opciones y resultados. Los enteros de operaciones aritméticas
tienen rangos iniciales acotados; Div/Rem usan divisor positivo para que el rango
sea demostrable. Los tipos se pueden ajustar en Propiedades.

Selecciona un nodo para ver operación, puertos, efectos y capacidades requeridas.
El formulario inferior modifica constantes según su tipo actual: enteros decimales,
`true`/`false`, texto literal o bytes hexadecimales sin espacios. El formulario
visual conserva hasta 131072 unidades UTF-16, suficientes para representar los
64 KiB de bytes como hexadecimal. El modelo limita las constantes a 64 KiB;
un texto que excede el límite se rechaza sin recortarlo.

Para modificar un puerto escribe, por ejemplo, `in 0 int -100 100` o
`out 0 bool` y elige Propiedades → Tipo de puerto. También admite `text` y `bytes`.
Con ningún nodo seleccionado se modifican puertos del grafo. Seleccionar una salida
y elegir Resultado desde salida seleccionada copia su tipo al resultado del grafo.
Conecta seleccionando una salida y después una entrada. El editor rechaza endpoints
inexistentes, tipos incompatibles y ciclos. Los cambios de tipos pueden dejar
conexiones previas inválidas: Validar muestra el diagnóstico y Guardar/Ejecutar
rechazan el documento hasta corregirlo.

Para crear un grafo escribe su nombre en el formulario y elige Crear grafo;
el documento pasa a ser un programa nativo. Siguiente grafo recorre sus definiciones.
Llamar grafo crea un Subgraph con la interfaz declarada del grafo cuyo nombre está
en el formulario. Duplicar operación conserva todos sus tipos, efectos y requisitos,
pero crea un ID nuevo y deja las entradas sin conectar.

Crear operación con contrato tipado abre un formulario multilínea de hasta 8192
bytes; el selector nativo lista todas las operaciones GIR. Completa los parámetros
posicionales en `operation=` y los puertos en `inputs=` / `outputs=`:

```text
operation=MakeRecord Point x
inputs=x:int(-100,100)
outputs=value:record(Point)
effects=
capabilities=
```

Los puertos se separan con `;`. `name:type` asigna IDs consecutivos desde cero;
`42,name:type` especifica el ID. Al editar se muestran IDs explícitos para
conservar interfaces importadas y sus conexiones. Los tipos
incluyen `int(min,max)`, `decimal(precision,scale)`, `float(error_ppb)`, `bigfloat(bits)`,
`record(nombre)`, `variant(nombre)`, `reference(nombre)`, `array(longitud,tipo)`,
`vector(longitud,tipo)`, `result(ok,error)` y los wrappers `slice`, `option`, `secret`,
`credential`, `unique`, `borrow`, `shared`, `state`, `atomic` y `versioned`.
El anidamiento se limita a 32. Los delimitadores no pueden formar parte de nombres;
los parámetros con nombre no contienen espacios.

Ejemplos de parámetros: `Subgraph nombre`, `Select true_graph false_graph`,
`Loop condition body max_iterations`, `Map body`,
`TaskSpawn body max_steps max_value_bytes`, `MakeRecord schema campo1,campo2`,
`MakeVariant schema tag`, `Match default_graph tag:graph tag:graph`,
`Truncate bits signed`, `StoreRead recurso campo1,campo2`,
`StoreSetRelation recurso relacion`, `StoreSetCredential recurso campo`,
`LocalExecute artefacto` y `RemoteExecute target artefacto`.
Las variantes Scoped/Hosted de Task usan el mismo orden. Para listas vacías usa `-`.
ConstInteger/ConstBool toman un valor, ConstText toma el resto de la línea y
ConstBytes toma hex o `-`.

Los efectos usan nombres GIR separados por `;`, por ejemplo `Storage;Network`.
Las capacidades usan `Clase,acción,recurso,scope;...`. Son requisitos declarados;
nunca son grants. Contratos → Editar contrato completo del nodo permite modificar
la operación y conserva su ID. Editar interfaz del grafo crea entradas y salidas
con la misma sintaxis, incluyendo interfaces con IDs no consecutivos.

Crear o actualizar schema abre, por ejemplo:

```text
name=Point
version=1
fields=1,x,required,int(-100,100)
```

Los campos se separan con `;`; cada uno declara tag, nombre, required/optional y tipo.
Una definición con el mismo nombre actualiza el schema y admite undo.
El registro admite hasta 256 schemas con 256 campos cada uno.

La API `GraphEditor::add_typed_node(Node)` admite todas las operaciones GIR y valida
la forma local antes de insertar. Los contratos de referencias, schemas, recursos
y autorización se verifican al guardar/ejecutar. Los formularios genéricos permiten
crear contratos de recursos y control estructurado. Los documentos de esta versión
no agregan bindings/policies externos ni conceden autoridad.

Deshacer/Rehacer conservan hasta 32 snapshots de documentos. El modelo admite
4096 nodos en total; se pueden crear hasta 256 grafos y 64 puertos por nodo tipado.

## Layout

Arrastra nodos para moverlos y usa la rueda para desplazarte verticalmente.
Guardar escribe `<documento>.layout` junto al archivo GIR/programa. Ese sidecar
binario `G0L` guarda posiciones por nombre de grafo e ID; no cambia la identidad GIR.
Puede eliminarse para recuperar la disposición automática. Un sidecar inválido
se ignora al abrir. Se limita a 1 MiB, 4096 posiciones, nombres de 1024 bytes y
coordenadas 0..100000 (Y desde 60). Cada archivo se reemplaza mediante un temporal
sin truncar el archivo anterior; GIR y layout son dos guardados separados.

## Depuración real y replay

Ejecutar y trazar ejecuta el grafo seleccionado y conserva la traza. Siguiente paso
y Paso anterior recorren eventos ya ejecutados; estos comandos son replay.

Depurar inicia un snapshot en un hilo del ejecutor y pausa antes del primer nodo.
Paso real permite ejecutar ese nodo y pausa antes del siguiente. Continuar avanza
hasta el próximo breakpoint; Pausar solicita detenerse antes del siguiente nodo;
Detener activa cancelación y despierta una espera pausada. El breakpoint se identifica
por nombre de grafo e ID, incluyendo llamadas anidadas. El panel muestra el nodo
pendiente y cuántos eventos ya ejecutaron; el nodo pendiente aún no produjo valores.
El editor bloquea cambios del documento mientras la sesión está activa.

Los valores sensibles usan la redacción normal del ejecutor. La traza tiene un
límite de 10000 eventos y los breakpoints, 4096. Los breakpoints duran la sesión
del editor. Detener no revierte efectos ya ejecutados. Una operación de host que
bloquea puede terminar después de pedir cancelación; cerrar la ventana no espera
indefinidamente a ese hilo.

## Verificación

`cargo test --offline --test editor` verifica edición GIR, validación, undo/redo,
operaciones de la toolbox, formularios tipados, layout hostil y pausa/step/stop/
breakpoints contra el ejecutor real. En Windows también crea una ventana oculta,
guarda y reabre el grafo 42 + 7, ejecuta el resultado 49, verifica BMP y sidecar,
usa los controles reales del debugger y crea un schema/nodo mediante los controles
multilínea y mensajes Win32.

`g0-editor --smoke-test preview.bmp graph.g0g` genera esos artefactos para inspección.
El renderer es Win32/GDI; no hay frontend web ni backend gráfico para otros sistemas.
