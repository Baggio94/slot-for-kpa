#!/usr/bin/env python3
"""Make assets/gb_board_open.png and assets/gb_chip.png from assets/gb_board.png:
the two big chips become empty footprints, and the right chip is the sprite
the core picker hops between them. Usage: make.py assets/gb_board.png assets"""
import sys
from PIL import Image, ImageDraw
import numpy as np
src=sys.argv[1]; out=sys.argv[2]
im=Image.open(src).convert('RGBA'); a=np.array(im).astype(np.int32)
# right chip with pins: the sprite
CHIP=(184,166,254,257)
chip=im.crop(CHIP)
# silkscreen boxes (outer) and centers
boxes=[(67,169,131,266),(181,163,257,260)]
# empty board: fill each box with PCB green sampled around the box, keep texture by mirroring a clean strip
board=im.copy(); d=ImageDraw.Draw(board)
green=tuple(int(v) for v in np.median(a[120:140,140:180,:3].reshape(-1,3),axis=0))+(255,)
cw,ch=CHIP[2]-CHIP[0],CHIP[3]-CHIP[1]
centres=[]
for (x0,y0,x1,y1) in boxes:
    d.rectangle((x0-2,y0-2,x1+3,y1+3),fill=green)
for i,(x0,y0,x1,y1) in enumerate(boxes):
    cx=(x0+x1)/2; cy=213.5
    centres.append((cx,cy))
    L=round(cx-cw/2); T=round(cy-ch/2)
    # silkscreen outline
    d.rounded_rectangle((L-3,T-3,L+cw+2,T+ch+2),radius=3,outline=(214,226,214,255),width=1)
    # pads where the pins go: copy pin columns geometry from the chip sprite
    ca=np.array(chip).astype(int)
    pin=(abs(ca[:,:,0]-ca[:,:,1])<25)&(abs(ca[:,:,1]-ca[:,:,2])<25)&(ca[:,:,0]>75)&(ca[:,:,3]>200)
    for yy in range(ch):
        for xx in range(cw):
            if pin[yy,xx]:
                board.putpixel((L+xx,T+yy),(200,165,80,255) if (yy%2==0 or True) else green)
board.putalpha(im.getchannel('A'))
board.save(f"{out}/gb_board_open.png"); chip.save(f"{out}/gb_chip.png")
open(f"{out}/centres.txt","w").write(repr(centres)+" chip %dx%d"%(cw,ch))
print(centres,cw,ch,green)
