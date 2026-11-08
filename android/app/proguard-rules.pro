# watchdog remote — R8 rules
#
# Keep the application entry points referenced only from the manifest.
-keep class app.watchdog.remote.MainActivity { *; }

# Kotlin metadata is used for reflection-free runtime checks; R8 strips it
# safely, but keep annotations readable in crash reports.
-keepattributes SourceFile,LineNumberTable
-renamesourcefileattribute SourceFile
