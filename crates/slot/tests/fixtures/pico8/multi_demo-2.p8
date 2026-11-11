pico-8 cartridge // http://www.pico-8.com
version 43
__lua__
-- a multicart that only finds its parts from the bbs
if stat(101) then
 load("#multi_demo_part")
else
 reload(0x8000,0,0x100,"multi_demo_part.p8")
 print("oh-no! no parts found")
end
function _draw() end
__label__
00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
