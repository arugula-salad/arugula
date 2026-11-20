# The core's Rust half finds these by name over JNI, which R8 can't see.
-keep class io.arugula.core.CoreError { <init>(java.lang.String); }
-keep interface io.arugula.core.Notify { void ready(); }
-keep class * implements io.arugula.core.Notify { void ready(); }
-keep, includedescriptorclasses class org.rustls.platformverifier.** { *; }
