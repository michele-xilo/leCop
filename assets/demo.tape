# Regenerate with: vhs assets/demo.tape (needs target/release/lecop and datasets/c_so100_press_red_button)
Output assets/demo.gif
Set Shell "bash"
Set FontSize 14
Set Width 1300
Set Height 800
Set Padding 20
Set Theme "Dracula"
Set TypingSpeed 60ms
Env PATH "/Users/michelexiloyannis/Documents/oss/lecop/target/release:/opt/homebrew/bin:/usr/bin:/bin"
Env PS1 "$ "
Hide
Type "cd datasets && clear"
Enter
Show
Type "lecop ./c_so100_press_red_button"
Sleep 600ms
Enter
Sleep 7s
