숨길 때
``` zsh
/usr/libexec/PlistBuddy -c 'Add :LSUIElement bool true' /Applications/[AppName].app/Contents/Info.plist
```

다시 보이고 싶을 때
``` zsh
/usr/libexec/PlistBuddy -c 'Delete :LSUIElement' /Applications/[Appname].app/Contents/Info.plist
```

> 걍 .app 패키지 내부 .plist 파일에서 Is App Agent → Yes로 해줘도 같음

