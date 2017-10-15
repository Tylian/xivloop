Good luck using this!
 
In order for it to function, you need two things.

1. libmp3lame as a static library somewhere on the path. On windows I just pasted mp3lame.lib into the root directory.
2. libmp3lame as a dynamic library beside the binary executable. I just paste libmp3lame.dll into target/debug/ etc etc.