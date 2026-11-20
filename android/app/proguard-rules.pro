# JNI entry points are called from Kotlin via System.loadLibrary; keep their names.
-keepclasseswithmembernames class * {
    native <methods>;
}
-keep class dev.grokrs.app.NativeBridge { *; }

# kotlinx.serialization keeps generated serializers reachable.
-keepattributes *Annotation*, InnerClasses
-dontnote kotlinx.serialization.**
-keepclassmembers class kotlinx.serialization.json.** { *** Companion; }
-keepclasseswithmembers class kotlinx.serialization.json.** { kotlinx.serialization.KSerializer serializer(...); }
-keep,includedescriptorclasses class dev.grokrs.app.**$$serializer { *; }
-keepclassmembers class dev.grokrs.app.** { *** Companion; }
-keepclasseswithmembers class dev.grokrs.app.** { kotlinx.serialization.KSerializer serializer(...); }