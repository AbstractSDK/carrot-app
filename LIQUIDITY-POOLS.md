
https://docs.osmosis.zone/osmosis-core/modules/concentrated-liquidity/#geometric-tick-spacing-with-additive-ranges

--> For providing


L is the virtual liquidity, which is the mix of tokens at the current price
P_l is the lower tick price
P_u is the upper tick price


Lx = (Dx \sqrt{P_u} \sqrt{P_l})/(\sqrt{P_u} - \sqrt{P_l})
Ly = Dy/(\sqrt{P_u} - \sqrt{P_l})


Then we compute dx and dy again which are the final amount we will provide

Dx = (L(\sqrt{U} - \sqrt{L}))/(\sqrt{U}\sqrt{L})$$ 


Dy = L*(\sqrt{p(i_c)} - \sqrt{p(i_l)})$$


