# Adriane JVM SDK

Java wrapper over `adriane-c-api` using JNA. Kotlin and Scala should consume the
same `ai.adriane.Adriane` class rather than adding a second native binding.

Set `ADRIANE_C_API_LIB` when the native library is not on `java.library.path`.

```bash
mvn -f sdks/jvm/pom.xml test-compile dependency:build-classpath -Dmdep.outputFile=target/classpath.txt
ADRIANE_C_API_LIB=$PWD/crates/target/debug/libadriane_c_api.dylib \
  java -cp "sdks/jvm/target/classes:sdks/jvm/target/test-classes:$(cat sdks/jvm/target/classpath.txt)" \
  ai.adriane.AdrianeSmoke
```
