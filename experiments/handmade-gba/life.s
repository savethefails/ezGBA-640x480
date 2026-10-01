@ Conway's Game of Life for the Game Boy Advance, in ARM, encoded by asm.py.
@
@ Mode 3: a 240x160 bitmap of 15-bit colour straight in VRAM. Each cell is 2x2 pixels, so
@ the world is 120x80. Each world is kept as one byte per cell, with a dead border one cell
@ wide all round, so no neighbour count ever has to ask whether it is at an edge: 122x82 bytes.
@ A live cell's byte is its age, 1 to 255, and its colour is taken from a 32-step ramp by age.
@
@ A reseeds. So does running for 1200 generations, so a settled world never sits still for long.

W       = 122                   @ bytes per row, border included
H       = 82
CELLS   = W * H
WORLD_A = 0x02000000            @ EWRAM
WORLD_B = 0x02002800
VRAM    = 0x06000000
IWRAM   = 0x03000000
COUNTER = 0x02005000            @ generations since boot, for run.py to time
IO      = 0x04000000
REG_VCOUNT  = 0x06
REG_KEYINPUT = 0x130
ROW_BYTES = 480                 @ one row of pixels, 240 x 2 bytes
GENERATIONS = 1200

@ ---- cartridge header ------------------------------------------------------------------------

        b       start
        .org    0x080000A0
        .ascii  "HANDMADELIFE"  @ title, twelve bytes
        .ascii  "CLFE"          @ game code
        .ascii  "01"            @ maker
        .byte   0x96            @ fixed
        .byte   0x00            @ unit code
        .byte   0x00            @ device type
        .byte   0, 0, 0, 0, 0, 0, 0
        .byte   0x00            @ version
        .byte   0x00            @ header checksum, filled in by build.py
        .byte   0, 0

@ ---- start -----------------------------------------------------------------------------------

start:
        ldr     r0, =IO
        ldr     r1, =0x0403     @ mode 3, BG2 on
        strh    r1, [r0]
?FAST   ldr     r1, =0x4317     @ WAITCNT: ROM 3/1 wait states, prefetch on
?FAST   add     r2, r0, #0x200
?FAST   strh    r1, [r2, #4]
?IWRAM  ldr     r0, =step       @ the step routine, pool and all, into IWRAM
?IWRAM  ldr     r1, =IWRAM
?IWRAM  ldr     r2, =(step_end - step) / 4
?IWRAM copy:
?IWRAM  ldr     r3, [r0], #4
?IWRAM  str     r3, [r1], #4
?IWRAM  subs    r2, r2, #1
?IWRAM  bne     copy
        ldr     r11, =0x1234567 @ the random state; reseeding mixes in the frame counter
        mov     r12, #0         @ frames since boot, counted at each wait for vblank

reseed:
        @ Clear both worlds, borders included: 0x5000 bytes, a word at a time.
        ldr     r0, =WORLD_A
        mov     r1, #0
        mov     r2, #0x1400
clear:
        str     r1, [r0], #4
        subs    r2, r2, #1
        bne     clear

        @ Mix the frame count into the seed, so each press of A gives a different world.
        eor     r11, r11, r12, lsl #7
        ldr     r9, =WORLD_A    @ r9 the world being read, r10 the one being written
        ldr     r10, =WORLD_B
        ldr     r3, =1664525    @ the LCG's multiplier and increment (Numerical Recipes)
        ldr     r4, =1013904223
        add     r0, r9, #W + 1  @ first interior cell
        mov     r5, #80
seed_row:
        mov     r6, #120
seed_cell:
        mla     r11, r3, r11, r4 @ rd may not be rm on the ARM7TDMI; rs is free to be
        mov     r7, r11, lsr #24
        cmp     r7, #80         @ about 31% alive
        movlo   r7, #1
        movhs   r7, #0
        strb    r7, [r0], #1
        subs    r6, r6, #1
        bne     seed_cell
        add     r0, r0, #2      @ over the right border and the next row's left
        subs    r5, r5, #1
        bne     seed_row
        ldr     r8, =GENERATIONS

@ ---- one generation --------------------------------------------------------------------------

generation:
?ROM    bl      step
?IWRAM  ldr     r0, =IWRAM
?IWRAM  mov     lr, pc          @ pc reads two instructions ahead: the one after the bx
?IWRAM  bx      r0
        ldr     r0, =COUNTER
        ldr     r1, [r0]
        add     r1, r1, #1
        str     r1, [r0]

        @ Swap the worlds.
        mov     r0, r9
        mov     r9, r10
        mov     r10, r0

        @ Wait for the start of vblank, so the world moves at most once a frame.
        ldr     r0, =IO
?SYNC wait_out:
?SYNC ldrh    r1, [r0, #REG_VCOUNT]
?SYNC cmp     r1, #160
?SYNC beq     wait_out
?SYNC wait_in:
?SYNC ldrh    r1, [r0, #REG_VCOUNT]
?SYNC cmp     r1, #160
?SYNC bne     wait_in
        add     r12, r12, #1

        add     r1, r0, #0x100  @ ldrh reaches 255 bytes, and the keys are 0x130 in
        ldrh    r1, [r1, #REG_KEYINPUT - 0x100]
        tst     r1, #1          @ A, low when held
        beq     reseed
        subs    r8, r8, #1
        beq     reseed
        b       generation

        .pool

@ One generation, read from r9 into r10. Position independent, so the same bytes run
@ from ROM or from a copy in IWRAM: branches are relative and its pool travels with it.
step:
        add     r0, r9, #W + 1  @ r0 reads, r1 writes, r2 draws
        add     r1, r10, #W + 1
        ldr     r2, =VRAM
        ldr     r3, =ramp
        mov     r4, #80
row:
        mov     r5, #120
cell:
        mov     r6, #0          @ live neighbours
        ldrb    r7, [r0, #-W - 1]
        cmp     r7, #0
        addne   r6, r6, #1
        ldrb    r7, [r0, #-W]
        cmp     r7, #0
        addne   r6, r6, #1
        ldrb    r7, [r0, #-W + 1]
        cmp     r7, #0
        addne   r6, r6, #1
        ldrb    r7, [r0, #-1]
        cmp     r7, #0
        addne   r6, r6, #1
        ldrb    r7, [r0, #1]
        cmp     r7, #0
        addne   r6, r6, #1
        ldrb    r7, [r0, #W - 1]
        cmp     r7, #0
        addne   r6, r6, #1
        ldrb    r7, [r0, #W]
        cmp     r7, #0
        addne   r6, r6, #1
        ldrb    r7, [r0, #W + 1]
        cmp     r7, #0
        addne   r6, r6, #1

        ldrb    r7, [r0], #1    @ this cell's age, and on to the next
        cmp     r7, #0
        beq     dead
        cmp     r6, #2          @ alive: two or three neighbours keep it, one year older
        cmpne   r6, #3
        movne   r7, #0
        bne     store
        cmp     r7, #255
        addlo   r7, r7, #1
        b       store
dead:
        cmp     r6, #3          @ dead: exactly three bring it to life
        moveq   r7, #1
store:
        strb    r7, [r1], #1

        cmp     r7, #31         @ colour by age, held at the ramp's last step
        movhi   r6, #31
        movls   r6, r7
        add     r6, r3, r6, lsl #1
        ldrh    r6, [r6]
        orr     r6, r6, r6, lsl #16
        str     r6, [r2, #ROW_BYTES] @ the cell's lower two pixels, then its upper two
        str     r6, [r2], #4

        subs    r5, r5, #1
        bne     cell
        add     r0, r0, #2
        add     r1, r1, #2
        add     r2, r2, #ROW_BYTES @ past the row of pixels the lower halves already drew
        subs    r4, r4, #1
        bne     row
        bx      lr
        .pool
step_end:


@ The age ramp, BGR555: newborn white, then yellow, orange, red, magenta, violet, and settling
@ into a deep blue for cells that have stopped changing. Entry 0 is the dead background.
ramp:
