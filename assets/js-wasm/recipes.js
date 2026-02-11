function recipeBook (itemObject) {
    
    const recipeMap = new Map();
 
    recipeMap.set(JSON.stringify({ cloth: false, feathers: false, honey: false, sticks: false, string: false }), ['', '']);

    recipeMap.set(JSON.stringify({ cloth: true, feathers: false, honey: false, sticks: false, string: false }), ['A "Disguise"',' Add 1 to escape roll'])
    recipeMap.set(JSON.stringify({ cloth: false, feathers: true, honey: false, sticks: false, string: false }), ['Was something killed? ', ' Opponent losses next turn. Trap activates on D20: 3, 5, 11, Std Detection. Two Feathers cards must be played together to activate effect.']);
    recipeMap.set(JSON.stringify({ cloth: false, feathers: false, honey: true, sticks: false, string: false }), ['Yum! ', ' Recover one keal means in 4 turns. Recover one keal means in 2 turns if 2 honey are played together.']);
    recipeMap.set(JSON.stringify({ cloth: false, feathers: false, honey: false, sticks: true, string: false }), ['Hit with it... ', ' Add 1 to attack roll. No dragons...']);
    recipeMap.set(JSON.stringify({ cloth: false, feathers: false, honey: false, sticks: false, string: true }), ['Tangle up the situation! ', ' Opponent losses 2 damage from their next attack roll. Two String cards must be played together to activate effect.']);

    recipeMap.set(JSON.stringify({ cloth: false, feathers: true, honey: false, sticks: true, string: false }), ['Atlatl: ', ' Wielder now has a ranged attack and can attack 1 movement stand away with Cenozoic D6 vs defender keal means.']);
    recipeMap.set(JSON.stringify({ cloth: true, feathers: false, honey: false, sticks: true, string: false }), ['Bag on stick: ', ' Wielder may move 3 reasonable items or 1 reasonable trap.']);
    recipeMap.set(JSON.stringify({ cloth: false, feathers: true, honey: false, sticks: false, string: true }), ['Fire Starter? ', ' Chance to destroy terrain feature. Win with a D6 vs D20.']);
    recipeMap.set(JSON.stringify({ cloth: true, feathers: false, honey: false, sticks: false, string: true }), ['Tunic: ', ' No damage is taken due to storm. Once effect activates, the items are wasted.']);
    recipeMap.set(JSON.stringify({ cloth: false, feathers: true, honey: true, sticks: false, string: false }), ['Sticky feather trap! ', ' Take 1 damage from targets rolls until they attempt to swim in or cross water (without a boat). Trap activates on D20: 3, 5, 11, Std Detection.']);
    recipeMap.set(JSON.stringify({ cloth: false, feathers: false, honey: true, sticks: true, string: false }), ['Sticky stick fingers? ', ' Take 1 item card from your opponents deck via random draw and place it in your deck.']);
    recipeMap.set(JSON.stringify({ cloth: false, feathers: false, honey: true, sticks: false, string: true }), ['Sweet candle: ', ' Prolongs day, move 1 soul at night. If movement causes contention, you attack first and add 1 to attack roll. Items are wasted after use.']);
    recipeMap.set(JSON.stringify({ cloth: true, feathers: false, honey: true, sticks: false, string: false }), ['Bee Bomb! ', ' After 3 diel cycles, all souls in the territory lose a keal means.']);
    recipeMap.set(JSON.stringify({ cloth: false, feathers: false, honey: false, sticks: true, string: true }), ['Snare Trap: ', ' Immobilize target for until it can be rescued. Trap activates on D20: 3, 5, 11, Std Detection.']);
    recipeMap.set(JSON.stringify({ cloth: true, feathers: true, honey: false, sticks: false, string: false }), ['Pillow: ', ' Recover 1 keal means at night in a basecamp. Fixture item, can be stolen if not moved after a basecamp is lost.']);

    recipeMap.set(JSON.stringify({ cloth: true, feathers: true, honey: false, sticks: true, string: false }), ['Lite training bag: ', ' Target is immobilized for 1 turn while training. Target gains 1 attack damage until wasted. After training, items are wasted']);
    recipeMap.set(JSON.stringify({ cloth: false, feathers: true, honey: false, sticks: true, string: true }), ['Basic bow: ', ' Wielder now has a ranged attack and can attack 1 movement stand (or basecamp) away with Cenozoic D6-D6 (or D6 against soul in basecamp) vs defender keal means. Wielder can also launch small things (i.e. Bee bomb).']);
    recipeMap.set(JSON.stringify({ cloth: true, feathers: true, honey: false, sticks: false, string: true }), ['Jacket: ', ' For the wielder, No damage is taken during storm. Adds 1 extra damage taken due to heat and subtracts 1 extra damage due to cold.']);
    recipeMap.set(JSON.stringify({ cloth: true, feathers: false, honey: false, sticks: true, string: true }), ['Hammock: ', ' Recover 1 keal means during day in a basecamp (3 total if Pillow is in the same basecamp). Fixture item, can be stolen if not moved after a basecamp is lost.']);
    recipeMap.set(JSON.stringify({ cloth: false, feathers: true, honey: true, sticks: true, string: false }), ['Stick-y feather pit trap? ', ' Target takes 1 damage. If target is not wasted, take 1 damage from targets rolls until they attempt to swim in or cross water (without a boat). Can be reset. Trap activates on D20: 3, 5, 11, 13 Std detection.']);
    recipeMap.set(JSON.stringify({ cloth: true, feathers: true, honey: true, sticks: false, string: false }), ['Chicken "Disguise" ', ' Buckawk!!! An even better chance to escape. Wielder rolls with a D20 to escape.']);
    recipeMap.set(JSON.stringify({ cloth: false, feathers: true, honey: true, sticks: false, string: true }), ['"Advanced" fire starter? ', ' Better chance to destroy a terrain feature. Win with D6 against D6-D6 with a retry.']);
    recipeMap.set(JSON.stringify({ cloth: true, feathers: false, honey: true, sticks: true, string: false }), ['Torch: ', ' Prolongs day, move up to 3 souls at night (must start and end at the same spot as the wielder). If movement causes contention, you attack first and add 1 to attack roll. Items are wasted after use.']);
    recipeMap.set(JSON.stringify({ cloth: false, feathers: false, honey: true, sticks: true, string: true }), ['Baited snare trap: ', ' immobilize target for 1 turn. Trap activates on D20: 3, 5, 11, 13, 17, 19 Std detection. Insect/Bug appearing things become trapped as soon they are in the same territory.']);
    recipeMap.set(JSON.stringify({ cloth: true, feathers: false, honey: true, sticks: false, string: true }), ['Tough Bundles for blade training! ', ' The target blade wielder is immobilized for 1 turn while training. Target gains 2 attack damage until wasted. After training, items are wasted.']);
    
    var recipe = recipeMap.get(JSON.stringify(itemObject));
    if (recipe == null) {
        return ''
    } else {
        return recipe
    }
}