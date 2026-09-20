export class Cart {
  private total = 0;

  constructor(private readonly carrier: Carrier, private readonly queue: Job[]) {}

  add(item: Item): void {
    this.total += item.price;
  }

  getTotal(): number {
    return this.total;
  }

  shippingCost(order: Order): number {
    if (order.isExpress) {
      return this.carrier.expressQuote(order) + order.weight * 1.5;
    } else {
      return this.carrier.standardQuote(order);
    }
  }

  drain(): number {
    let handled = 0;
    while (this.queue.pop() !== undefined) {
      handled += 1;
    }
    return handled;
  }
}
