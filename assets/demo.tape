# Regenerate with: vhs assets/demo.tape (needs target/release/lecop and datasets/c_so100_press_red_button)
Output assets/demo.gif
Set Shell "bash"
Set FontSize 15
Set Width 1300
Set Height 420
Set Padding 20
Set Theme "Dracula"
Set TypingSpeed 40ms
Env PATH "./target/release:/opt/homebrew/bin:/usr/bin:/bin"
Env PS1 "$ "
Hide
Type "clear"
Enter
Show
Type "lecop --select frame_count,video_frame_count,stats_mismatch datasets/c_so100_press_red_button"
Sleep 600ms
Enter
Sleep 6s
