A linux program on hyprland arch linux base for playing games, my idea for this project is "Make All in one laucnher more easiar", Optimize and support for my another project like `SomeShell`


### Feature
I want to make it easiar in user interface as much possible, while still can customize or config anything like what proton want to use, what launch argrument or something like that because game on linux are still need own specific setup for best optimize, but mainly aim is clean interface, few click play game.
- Auto match Fetch game thumbnail by game name like lutris *Will ask user when setup game is it correct*.
- Game achievement *Question about this, can possible to make it?*
- Playtime track
- Version build check support via steamdb
- Auto custom for PC&Laptop *like laptop that has battery bypass or laptop gamemode*
- optimize as much possible, game launcher are just game launcher.
- Auto tags & categiries
### Games support
**Online Games**
- Genshin impact
- Zenless zone zero
*support for these on start, will add other online/gacha game later*
about these gacha online game, i want to make it can auto auto update and installing like official laucher or like `TwintailLauncher` that i use for play these games currently, these online games should check update everytime when launch the program to avoid the version mismatch
*For these gacha game, make it can use XXMI mod skin too*

**Offline Games**
A single player game, manually setup but easy to set.

**Co-op Games**
- R.E.P.O. via steam, this game
- Other game that need steam to launch, *use steam for playing Co-op* read `LUTRIS_SETUP.md`
steam on linux are still need to config a few config to playing game, and some Co-op online game like REPO are need to use steam
Question: how to make steam game work on our launcher or a game that need to use steam to play co-op?

**Minecraft support**
currently i use prism launcher, it very good and very customizable, but the app design are not that good and not suit to my current hyprland theme, so i want function and feature like prism launcher, but redesign it to suit our `SomeLauncher`
### App design & theme
The app design user interface Full game image title game and one play button, but while our launcher had many game support, should make difference layout design.
- Main design on my opinion: Left panel for navigate which page to go and setting, on  the left top are logo *leave emthy, i don't have it yet*  
- Homepage: homepage user interface on startup to easiar see game currently have or recent play, simple and clean, hide game that not usually play 
- Online games page: genshin and zenless zone zero, both game are from hoyoverse there launcher game are had full image *include animated* artwork for game version present, the layout design should be FULL image game artwork, title game, play button and setting button, playtime and game version,  event, and hoyolab link, had right dock for switch to other game like zenless zone zero, use same layout
- Offline games page *include co-op or game that need steam, just tags or label icon steam*:  design game libary, separate game each categories, main highlight on top like recent play game or favorite game, my thought is make it look like netflix ui that need to click on game and will show the full image game artwork, after select game, the layout are simliar to online game page or PS5 ui when select game to play, show playtime and achievement progress
- Minecraft page: this is hard part i think, can you design ux/ui for these part? i will review and feedback cause i like how simple and customizable of prism launcher are, but it look too old and it hard to use on first time.

Theme and how it should look
- use glassmorphism as main design
- focus on animation and seamless motion

i don't mind about how hard on setup game, but when finish setup should one click play, that are my target.
